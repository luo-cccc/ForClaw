use std::sync::{Arc, Mutex};

use crate::compaction::{compact_messages, should_compact, CompactionConfig};
use crate::context_window_guard::{
    evaluate_context_window, ContextWindowInfo, ContextWindowSource,
};
use crate::execution_plan::{
    AgentCheckpoint, CheckpointPhase, ExecutionPlan, ExecutionStep, PlanStatus, ProviderUsageSummary,
    ResumePolicy, StepEvidence, StepFailureAction, StepStatus,
};
use crate::provider::{LlmMessage, LlmRequest, Provider, StreamEvent};
use crate::recovery::{
    classify_failure, classify_failure_kind, map_failure_to_recovery, redact_sensitive,
    RecoveryContext, RecoveryDecision, RuntimeCallRecord,
    RuntimeCallStatus, RuntimeCallType,
};
use crate::router::{classify_intent, Intent};
use crate::tool_executor::{ToolExecution, ToolExecutor, ToolHandler};
use crate::tool_registry::{ToolFilter, ToolRegistry, ToolSideEffectLevel};

/// Events emitted during agent loop execution to the UI layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum AgentLoopEvent {
    #[serde(rename = "intent")]
    Intent {
        intent: String,
        #[serde(default)]
        confidence: f32,
        #[serde(default)]
        evidence: Vec<String>,
    },
    #[serde(rename = "thinking")]
    Thinking,
    #[serde(rename = "text_chunk")]
    TextChunk { content: String },
    #[serde(rename = "tool_call_start")]
    ToolCallStart {
        tool: String,
        args: serde_json::Value,
    },
    #[serde(rename = "tool_call_end")]
    ToolCallEnd { tool: String, result: ToolExecution },
    #[serde(rename = "doom_loop_warning")]
    DoomLoopWarning { tool: String },
    #[serde(rename = "tool_inventory")]
    ToolInventory { tools: Vec<String>, generation: u64 },
    #[serde(rename = "provider_guard")]
    ProviderGuard {
        allowed: bool,
        model: String,
        estimated_input_tokens: u64,
        requested_output_tokens: u64,
    },
    #[serde(rename = "context_window")]
    ContextWindow {
        tokens: u64,
        estimated_input: u64,
        requested_output: u64,
        should_warn: bool,
        should_block: bool,
    },
    #[serde(rename = "compaction")]
    Compaction {
        before_tokens: u64,
        after_tokens: u64,
        compacted_count: usize,
        #[serde(default)]
        compaction_kind: String,
        #[serde(default)]
        tokens_saved_by_truncation: u64,
        #[serde(default)]
        boundary_summary: String,
    },
    #[serde(rename = "plan_started")]
    PlanStarted { plan_id: String, steps: Vec<String> },
    #[serde(rename = "step_started")]
    StepStarted {
        step_id: String,
        index: usize,
        goal: String,
    },
    #[serde(rename = "step_completed")]
    StepCompleted {
        step_id: String,
        evidence: Vec<String>,
    },
    #[serde(rename = "step_blocked")]
    StepBlocked {
        step_id: String,
        reason: String,
        context_summary: Vec<String>,
        recovery_suggestion: String,
    },
    #[serde(rename = "step_failed")]
    StepFailed {
        step_id: String,
        reason: String,
        action: String,
        input_context_summary: Vec<String>,
        recovery_suggestion: String,
    },
    #[serde(rename = "step_transition")]
    StepTransition {
        step_id: String,
        from: String,
        to: String,
    },
    #[serde(rename = "plan_completed")]
    PlanCompleted {
        plan_id: String,
        steps_completed: usize,
        steps_failed: usize,
        steps_skipped: usize,
    },
    #[serde(rename = "failure_bundle")]
    FailureBundle {
        run_id: String,
        failed_step: String,
        error_kind: String,
        completed_steps: Vec<String>,
        suggested_action: String,
    },
    #[serde(rename = "recovery_bundle")]
    RecoveryBundle {
        completed_steps: Vec<StepEvidence>,
        failed_step: String,
        failure_kind: String,
        input_context_summary: String,
        runtime_calls: Vec<RuntimeCallRecord>,
        suggested_action: String,
        user_choice_required: bool,
    },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "complete")]
    Complete {
        rounds: u32,
        tool_calls: u32,
        tokens_used: u64,
        cached_tokens: Option<u64>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        ttft_ms: Option<u64>,
        total_provider_duration_ms: u64,
        first_provider_call_ms: u64,
        last_provider_call_ms: u64,
    },
    #[serde(rename = "runtime_call_record")]
    RuntimeCallRecord { record: RuntimeCallRecord },
    #[serde(rename = "run_report")]
    RunReport { report: crate::run_trace::AgentRunReport },
}

/// Configuration for agent loop execution.
#[derive(Debug, Clone)]
pub struct AgentLoopConfig {
    pub max_rounds: u32,
    pub system_prompt: String,
    pub context_limit_tokens: Option<u64>,
    pub tool_filter: Option<ToolFilter>,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_rounds: 10,
            system_prompt: String::new(),
            context_limit_tokens: None,
            tool_filter: None,
        }
    }
}

/// Event callback type supplied by the host runtime.
pub type EventCallback = Arc<dyn Fn(AgentLoopEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ProviderCallContext {
    pub round: u32,
    pub provider: String,
    pub model: String,
    pub estimated_input_tokens: u64,
    pub requested_output_tokens: u64,
    pub message_count: usize,
    pub tool_count: usize,
    pub stream: bool,
}

pub type ProviderCallGuard = Arc<dyn Fn(ProviderCallContext) -> Result<(), String> + Send + Sync>;

/// The core agent execution loop.
/// Generic over Provider and ToolHandler — fully testable with mocks.
/// Ported from Claw Code `ConversationRuntime<C,T>` pattern.
pub struct AgentLoop<P: Provider, H: ToolHandler> {
    pub config: AgentLoopConfig,
    pub provider: Arc<P>,
    pub executor: ToolExecutor<H>,
    pub messages: Vec<LlmMessage>,
    pub on_event: Option<EventCallback>,
    pub provider_call_guard: Option<ProviderCallGuard>,
    pub last_usage: Option<crate::provider::UsageInfo>,
    pub ttft_ms: Option<u64>,
    pub total_provider_duration_ms: u64,
    pub first_provider_call_ms: u64,
    pub last_provider_call_ms: u64,
    /// Collected runtime call records for report generation.
    pub runtime_call_records: Mutex<Vec<RuntimeCallRecord>>,
}

impl<P: Provider, H: ToolHandler> AgentLoop<P, H> {
    pub fn new(
        config: AgentLoopConfig,
        provider: Arc<P>,
        registry: ToolRegistry,
        handler: H,
    ) -> Self {
        Self {
            config,
            provider,
            executor: ToolExecutor::new(registry, handler),
            messages: Vec::new(),
            on_event: None,
            provider_call_guard: None,
            last_usage: None,
            ttft_ms: None,
            total_provider_duration_ms: 0,
            first_provider_call_ms: 0,
            last_provider_call_ms: 0,
            runtime_call_records: Mutex::new(Vec::new()),
        }
    }

    pub fn set_event_callback(&mut self, cb: EventCallback) {
        self.on_event = Some(cb);
    }

    pub fn set_provider_call_guard(&mut self, guard: ProviderCallGuard) {
        self.provider_call_guard = Some(guard);
    }

    fn emit(&self, event: AgentLoopEvent) {
        if let AgentLoopEvent::RuntimeCallRecord { ref record } = event {
            if let Ok(mut records) = self.runtime_call_records.lock() {
                records.push(record.clone());
            }
        }
        if let Some(ref cb) = self.on_event {
            cb(event);
        }
    }

    /// Build and emit an `AgentRunReport` from the current plan state.
    fn emit_run_report(
        &self,
        plan: &ExecutionPlan,
        completed_step_evidences: &[StepEvidence],
    ) {
        let runtime_calls = self
            .runtime_call_records
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let report = crate::run_trace::build_agent_run_report(
            plan.plan_id.clone(),
            plan,
            &runtime_calls,
            completed_step_evidences,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        self.emit(AgentLoopEvent::RunReport { report });
    }

    /// Build and emit a `RecoveryBundle` event from a step failure.
    fn emit_recovery_bundle(
        &self,
        _plan_id: &str,
        step: &ExecutionStep,
        error: &str,
        completed_steps: &[StepEvidence],
    ) {
        let failure_kind = classify_failure_kind(error, None);
        let recovery_decision =
            map_failure_to_recovery(&failure_kind, &RecoveryContext::default());
        let user_choice_required = matches!(
            recovery_decision,
            RecoveryDecision::SurfaceUserChoice | RecoveryDecision::RequestApproval
        );
        let failure_kind_str = serde_json::to_string(&failure_kind)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let suggested_action_str = serde_json::to_string(&recovery_decision)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        self.emit(AgentLoopEvent::RecoveryBundle {
            completed_steps: completed_steps.to_vec(),
            failed_step: step.step_id.clone(),
            failure_kind: failure_kind_str,
            input_context_summary: step.goal.clone(),
            runtime_calls: vec![],
            suggested_action: suggested_action_str,
            user_choice_required,
        });
    }

    /// Add a user message to the conversation.
    pub fn add_user_message(&mut self, content: String) {
        self.messages.push(LlmMessage {
            role: "user".into(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    /// Estimate total tokens in the current conversation.
    pub fn estimate_tokens(&self) -> u64 {
        self.provider.estimate_tokens(&self.messages)
            + (self.config.system_prompt.chars().count() as u64 / 3)
    }

    /// Build the available tools list filtered by intent.
    pub fn build_tools(&self, intent: &Intent) -> Vec<serde_json::Value> {
        let filter = self.config.tool_filter.clone().unwrap_or(ToolFilter {
            intent: Some(intent.clone()),
            max_side_effect_level: Some(ToolSideEffectLevel::Write),
            include_requires_approval: true,
            include_disabled: false,
            required_tags: Vec::new(),
            allowed_names: Vec::new(),
        });
        let registry = self.executor.registry.blocking_lock();
        registry.to_effective_openai_tools(&filter, &self.executor.permission_policy)
    }

    async fn build_tools_async(&self, intent: &Intent) -> Vec<serde_json::Value> {
        let filter = self.config.tool_filter.clone().unwrap_or(ToolFilter {
            intent: Some(intent.clone()),
            max_side_effect_level: Some(ToolSideEffectLevel::Write),
            include_requires_approval: true,
            include_disabled: false,
            required_tags: Vec::new(),
            allowed_names: Vec::new(),
        });
        let registry = self.executor.registry.lock().await;
        registry.to_effective_openai_tools(&filter, &self.executor.permission_policy)
    }

    /// The main execution loop.
    ///
    /// 1. Classify intent → filter tools
    /// 2. While rounds < max: call LLM with streaming → execute tool calls → append results
    /// 3. Return the final text from the last assistant response
    pub async fn run(
        &mut self,
        user_message: &str,
        has_lorebook: bool,
        has_outline: bool,
    ) -> Result<String, String> {
        // Phase 1: Classify intent
        let classification = classify_intent(user_message, has_lorebook, has_outline);
        self.emit(AgentLoopEvent::Intent {
            intent: format!("{:?}", classification.intent),
            confidence: classification.confidence,
            evidence: classification.evidence.clone(),
        });
        // Emit classification detail as metadata
        let intent = classification.intent.clone();

        // Phase 2: Build tools for this intent
        let tools = self.build_tools_async(&intent).await;
        let has_tools = !tools.is_empty();

        let tool_names: Vec<String> = tools
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
            .collect();
        let generation = {
            let registry = self.executor.registry.lock().await;
            registry.generation()
        };
        self.emit(AgentLoopEvent::ToolInventory {
            tools: tool_names,
            generation,
        });

        // Phase 3: Execution rounds
        let mut rounds = 0u32;
        let mut total_tool_calls = 0u32;
        let mut final_text = String::new();

        self.emit(AgentLoopEvent::Thinking);

        while rounds < self.config.max_rounds {
            // Build LLM request
            let request = LlmRequest {
                messages: self.messages.clone(),
                tools: if has_tools { Some(tools.clone()) } else { None },
                temperature: Some(0.7),
                max_tokens: Some(4096),
                system: Some(self.config.system_prompt.clone()),
                stream: true,
            };
            let requested_output_tokens = request.max_tokens.unwrap_or(4096) as u64;
            let guard = evaluate_context_window(
                ContextWindowInfo {
                    tokens: self
                        .config
                        .context_limit_tokens
                        .unwrap_or_else(|| self.provider.context_window_tokens()),
                    reference_tokens: None,
                    source: if self.config.context_limit_tokens.is_some() {
                        ContextWindowSource::Env
                    } else {
                        ContextWindowSource::ModelMetadata
                    },
                },
                self.estimate_tokens(),
                requested_output_tokens,
            );
            self.emit(AgentLoopEvent::ContextWindow {
                tokens: guard.tokens,
                estimated_input: guard.estimated_input_tokens,
                requested_output: guard.requested_output_tokens,
                should_warn: guard.should_warn,
                should_block: guard.should_block,
            });
            if guard.should_block {
                let message = guard
                    .message
                    .unwrap_or_else(|| "Model context window too small".to_string());
                self.emit(AgentLoopEvent::Error {
                    message: message.clone(),
                });
                return Err(message);
            }
            if let Some(message) = guard.message.filter(|_| guard.should_warn) {
                self.emit(AgentLoopEvent::Error { message });
            }

            if let Some(provider_call_guard) = &self.provider_call_guard {
                let model = self
                    .provider
                    .models()
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| "unknown".to_string());
                let estimated_input_tokens = self.provider.estimate_tokens(&request.messages)
                    + request
                        .system
                        .as_ref()
                        .map(|system| system.chars().count() as u64 / 3)
                        .unwrap_or(0)
                    + request
                        .tools
                        .as_ref()
                        .map(|tools| tools.len() as u64 * 256)
                        .unwrap_or(0);
                let requested_output_tokens_val = requested_output_tokens;

                let guard_result = provider_call_guard(ProviderCallContext {
                    round: rounds + 1,
                    provider: self.provider.name().to_string(),
                    model: model.clone(),
                    estimated_input_tokens,
                    requested_output_tokens: requested_output_tokens_val,
                    message_count: request.messages.len(),
                    tool_count: request.tools.as_ref().map(|tools| tools.len()).unwrap_or(0),
                    stream: request.stream,
                });

                let allowed = guard_result.is_ok();
                self.emit(AgentLoopEvent::ProviderGuard {
                    allowed,
                    model,
                    estimated_input_tokens,
                    requested_output_tokens: requested_output_tokens_val,
                });

                if let Err(message) = guard_result {
                    self.emit(AgentLoopEvent::Error {
                        message: message.clone(),
                    });
                    return Err(message);
                }
            }

            // Call LLM with streaming — forward text chunks to UI
            let event_cb = self.on_event.clone();
            let call_start = std::time::Instant::now();
            let ttft_cell = std::sync::Arc::new(std::sync::Mutex::new(None::<u64>));
            let ttft_clone = ttft_cell.clone();
            let response = self
                .provider
                .stream_call(
                    request,
                    Box::new(move |ev| {
                        if let StreamEvent::TextDelta { content } = &ev {
                            let mut ttft = ttft_clone.lock().unwrap();
                            if ttft.is_none() {
                                *ttft = Some(call_start.elapsed().as_millis() as u64);
                            }
                            if let Some(ref cb) = &event_cb {
                                cb(AgentLoopEvent::TextChunk {
                                    content: content.clone(),
                                });
                            }
                        }
                    }),
                )
                .await
                .inspect_err(|e| {
                    self.emit(AgentLoopEvent::Error { message: e.clone() });
                })?;
            let call_duration_ms = call_start.elapsed().as_millis() as u64;
            let ttft_ms = *ttft_cell.lock().unwrap();
            self.total_provider_duration_ms += call_duration_ms;
            self.ttft_ms = ttft_ms.or(Some(call_duration_ms));
            if self.first_provider_call_ms == 0 {
                self.first_provider_call_ms = call_duration_ms;
            }
            self.last_provider_call_ms = call_duration_ms;
            self.last_usage = response.usage.clone();

            let response_tool_calls = response.tool_calls.unwrap_or_default();

            // Emit RuntimeCallRecord for provider call
            let provider_record = RuntimeCallRecord {
                call_id: format!("provider-call-{}-{}", rounds, rounds),
                call_type: RuntimeCallType::ProviderCall,
                step_id: format!("round-{}", rounds),
                timestamp_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                input_redacted_summary: serde_json::to_string(&redact_sensitive(
                    &serde_json::json!({
                        "model": self.provider.models().into_iter().next().unwrap_or_else(|| "unknown".to_string()),
                        "message_count": self.messages.len(),
                        "tool_count": response_tool_calls.len(),
                    }),
                ))
                .unwrap_or_default(),
                output_summary: format!(
                    "tokens_in={:?} tokens_out={:?} ttft={:?}ms",
                    response.usage.as_ref().map(|u| u.input_tokens),
                    response.usage.as_ref().map(|u| u.output_tokens),
                    ttft_ms
                ),
                duration_ms: call_duration_ms,
                status: RuntimeCallStatus::Success,
                remediation_code: None,
            };
            self.emit(AgentLoopEvent::RuntimeCallRecord {
                record: provider_record,
            });

            // No tool calls → done
            if response_tool_calls.is_empty() {
                final_text = response.content.unwrap_or_default();
                self.messages.push(LlmMessage {
                    role: "assistant".into(),
                    content: Some(final_text.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
                break;
            }

            // Execute each tool call
            let mut assistant_tool_calls = Vec::new();
            for tc in &response_tool_calls {
                total_tool_calls += 1;

                let args: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);

                self.emit(AgentLoopEvent::ToolCallStart {
                    tool: tc.function.name.clone(),
                    args: args.clone(),
                });

                let execution = self.executor.execute(&tc.function.name, args).await;

                // Check for doom loop
                if execution
                    .error
                    .as_ref()
                    .map(|e| e.contains("DOOM LOOP"))
                    .unwrap_or(false)
                {
                    self.emit(AgentLoopEvent::DoomLoopWarning {
                        tool: tc.function.name.clone(),
                    });
                }

                self.emit(AgentLoopEvent::ToolCallEnd {
                    tool: tc.function.name.clone(),
                    result: execution.clone(),
                });

                // Emit RuntimeCallRecord for tool call
                let tool_status = if let Some(ref err) = execution.error {
                    if execution
                        .remediation
                        .iter()
                        .any(|r| r.code == "approval_required")
                    {
                        RuntimeCallStatus::Blocked {
                            reason: err.clone(),
                        }
                    } else {
                        RuntimeCallStatus::Failed {
                            reason: err.clone(),
                        }
                    }
                } else {
                    RuntimeCallStatus::Success
                };
                let tool_record = RuntimeCallRecord {
                    call_id: format!("tool-call-{}-{}", total_tool_calls, total_tool_calls),
                    call_type: RuntimeCallType::ToolCall,
                    step_id: format!("round-{}", rounds),
                    timestamp_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    input_redacted_summary: serde_json::to_string(&redact_sensitive(&execution.input))
                        .unwrap_or_default(),
                    output_summary: serde_json::to_string(&execution.output).unwrap_or_default(),
                    duration_ms: execution.duration_ms,
                    status: tool_status.clone(),
                    remediation_code: execution.remediation.first().map(|r| r.code.clone()),
                };
                self.emit(AgentLoopEvent::RuntimeCallRecord {
                    record: tool_record,
                });

                // Add tool result to conversation
                self.messages.push(LlmMessage {
                    role: "tool".into(),
                    content: Some(execution.output.to_string()),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    name: Some(tc.function.name.clone()),
                });

                assistant_tool_calls.push(tc.clone());
            }

            // Add assistant message (the one that requested the tool calls)
            self.messages.push(LlmMessage {
                role: "assistant".into(),
                content: match response.content {
                    Some(ref c) if !c.is_empty() => Some(c.clone()),
                    _ => None,
                },
                tool_calls: Some(assistant_tool_calls),
                tool_call_id: None,
                name: None,
            });

            rounds += 1;

            // Auto-compaction check after each tool-execution round
            let compaction_cfg = CompactionConfig::default();
            if should_compact(&self.messages, &self.config.system_prompt, &compaction_cfg) {
                let before = self.estimate_tokens();
                match compact_messages(&self.messages, &compaction_cfg, &*self.provider).await {
                    Ok((compacted, report)) => {
                        self.messages = compacted;
                        self.emit(AgentLoopEvent::Compaction {
                            before_tokens: before,
                            after_tokens: report.tokens_after,
                            compacted_count: report.compacted_count,
                            compaction_kind: format!("{:?}", report.kind),
                            tokens_saved_by_truncation: report.tokens_saved_by_tool_truncation,
                            boundary_summary: report.boundary_summary.clone(),
                        });
                    }
                    Err(e) => {
                        self.emit(AgentLoopEvent::Error {
                            message: format!("Compaction failed: {}", e),
                        });
                        // Continue without compaction — better than crashing
                    }
                }
            }
        }

        // Check max rounds exceeded
        if rounds >= self.config.max_rounds && final_text.is_empty() {
            let msg = format!(
                "Reached max rounds ({}) without final response",
                self.config.max_rounds
            );
            self.emit(AgentLoopEvent::Error {
                message: msg.clone(),
            });
            return Err(msg);
        }

        let usage = self.last_usage.clone();
        if let Some(ref usage) = usage {
            let total_chars: usize = self
                .messages
                .iter()
                .map(|m| m.content.as_ref().map(|c| c.chars().count()).unwrap_or(0))
                .sum::<usize>()
                + self.config.system_prompt.chars().count();
            let model = self
                .provider
                .models()
                .into_iter()
                .next()
                .unwrap_or_else(|| "unknown".to_string());
            crate::budget_calibration::record_usage(&model, usage.input_tokens, total_chars);
        }
        self.emit(AgentLoopEvent::Complete {
            rounds,
            tool_calls: total_tool_calls,
            tokens_used: self.estimate_tokens(),
            cached_tokens: usage.as_ref().and_then(|u| u.cached_tokens),
            input_tokens: usage.as_ref().map(|u| u.input_tokens),
            output_tokens: usage.as_ref().map(|u| u.output_tokens),
            ttft_ms: self.ttft_ms,
            total_provider_duration_ms: self.total_provider_duration_ms,
            first_provider_call_ms: self.first_provider_call_ms,
            last_provider_call_ms: self.last_provider_call_ms,
        });

        Ok(final_text)
    }

    pub async fn run_with_plan(
        &mut self,
        plan: &mut ExecutionPlan,
        user_message: &str,
    ) -> Result<String, String> {
        self.run_plan_inner(plan, user_message, false).await
    }

    /// Resume an incomplete plan from the first non-terminal step.
    /// Completed, Failed, and Skipped steps are skipped.
    pub async fn resume_plan(
        &mut self,
        plan: &mut ExecutionPlan,
        user_message: &str,
    ) -> Result<String, String> {
        self.run_plan_inner(plan, user_message, true).await
    }

    async fn run_plan_inner(
        &mut self,
        plan: &mut ExecutionPlan,
        user_message: &str,
        is_resume: bool,
    ) -> Result<String, String> {
        let step_goals: Vec<String> = plan.steps.iter().map(|s| s.goal.clone()).collect();
        if !is_resume {
            self.emit(AgentLoopEvent::PlanStarted {
                plan_id: plan.plan_id.clone(),
                steps: step_goals,
            });
        }
        plan.status = PlanStatus::Running;

        let mut final_text = String::new();
        let mut steps_completed = 0usize;
        let mut steps_failed = 0usize;
        let mut steps_skipped = 0usize;
        let mut completed_step_ids: Vec<String> = Vec::new();
        let mut completed_step_evidences: Vec<StepEvidence> = Vec::new();

        for step in &mut plan.steps {
            // Skip terminal steps on resume
            if step.is_terminal() {
                match &step.status {
                    StepStatus::Completed { .. } => steps_completed += 1,
                    StepStatus::Failed { .. } => steps_failed += 1,
                    StepStatus::Skipped { .. } => steps_skipped += 1,
                    _ => {}
                }
                continue;
            }

            // Lifecycle: Ready -> Running
            let previous_status = format!("{:?}", step.status);
            step.status = StepStatus::Ready;
            self.emit(AgentLoopEvent::StepTransition {
                step_id: step.step_id.clone(),
                from: previous_status,
                to: "ready".to_string(),
            });

            step.status = StepStatus::Running;
            self.emit(AgentLoopEvent::StepStarted {
                step_id: step.step_id.clone(),
                index: step.index,
                goal: step.goal.clone(),
            });

            // Checkpoint: step started
            let _step_started_checkpoint = AgentCheckpoint {
                checkpoint_id: format!("{}-{}-started", plan.plan_id, step.step_id),
                task_id: plan.task_id.clone(),
                plan_id: plan.plan_id.clone(),
                step_id: step.step_id.clone(),
                phase: CheckpointPhase::StepStarted,
                input_hash: String::new(),
                context_hash: String::new(),
                artifact_refs: vec![],
                tool_effects: vec![],
                provider_usage: None,
                budget_spent: 0,
                approval_refs: vec![],
                resume_policy: ResumePolicy::Rerun,
            };

            // If a StepContract exists, validate it and set up the executor whitelist.
            if let Some(ref contract) = step.contract {
                self.executor
                    .set_allowed_tools(Some(contract.allowed_tools.clone()));
            } else {
                self.executor.set_allowed_tools(Some(step.allowed_tools.clone()));
            }

            // Constrain tool filter to this step's max_side_effect and allowed_tools
            let saved_filter = self.config.tool_filter.clone();
            let allowed_names = step
                .contract
                .as_ref()
                .map(|c| c.allowed_tools.clone())
                .unwrap_or_else(|| step.allowed_tools.clone());
            self.config.tool_filter = Some(crate::tool_registry::ToolFilter {
                intent: saved_filter.as_ref().and_then(|f| f.intent.clone()),
                include_requires_approval: false,
                include_disabled: false,
                max_side_effect_level: Some(
                    step.contract
                        .as_ref()
                        .map(|c| c.max_side_effect)
                        .unwrap_or(step.max_side_effect),
                ),
                required_tags: Vec::new(),
                allowed_names,
            });

            let step_result = self.run(user_message, true, true).await;
            self.config.tool_filter = saved_filter;
            self.executor.set_allowed_tools(None);

            match step_result {
                Ok(text) => {
                    // Check required evidence from contract
                    if let Some(ref contract) = step.contract {
                        let mut missing_evidence: Vec<String> = Vec::new();
                        for required in &contract.success_evidence_required {
                            let has_evidence = step.evidence.as_ref().map_or(false, |e| {
                                e.artifact_refs.iter().any(|a| a.contains(required))
                                    || e.tool_executions.iter().any(|t| t.contains(required))
                                    || e.context_refs.iter().any(|c| c.contains(required))
                            });
                            if !has_evidence {
                                missing_evidence.push(required.clone());
                            }
                        }
                        if !missing_evidence.is_empty() {
                            let reason = format!(
                                "Step completed but missing required evidence types: {:?}",
                                missing_evidence
                            );
                            steps_failed += 1;
                            step.status = StepStatus::Failed {
                                reason: reason.clone(),
                                input_context_summary: vec![step.goal.clone()],
                                recovery_suggestion: format!(
                                    "Ensure the step produces artifacts matching: {:?}",
                                    contract.success_evidence_required
                                ),
                            };
                            self.emit(AgentLoopEvent::StepFailed {
                                step_id: step.step_id.clone(),
                                reason: reason.clone(),
                                action: "abort".into(),
                                input_context_summary: vec![step.goal.clone()],
                                recovery_suggestion: format!(
                                    "Ensure the step produces artifacts matching: {:?}",
                                    contract.success_evidence_required
                                ),
                            });
                            plan.status = PlanStatus::Failed {
                                failed_step: step.step_id.clone(),
                                reason: reason.clone(),
                            };
                            self.emit(AgentLoopEvent::PlanCompleted {
                                plan_id: plan.plan_id.clone(),
                                steps_completed,
                                steps_failed,
                                steps_skipped,
                            });
                            self.emit_run_report(plan, &completed_step_evidences);
                            return Err(reason);
                        }
                    }

                    // Generate StepEvidence from usage info
                    let evidence = StepEvidence {
                        step_id: step.step_id.clone(),
                        artifact_refs: vec![text.clone()],
                        tool_executions: vec![],
                        provider_usage: self.last_usage.as_ref().map(|usage| ProviderUsageSummary {
                            model: self
                                .provider
                                .models()
                                .into_iter()
                                .next()
                                .unwrap_or_else(|| "unknown".to_string()),
                            prompt_tokens: usage.input_tokens as u32,
                            completion_tokens: usage.output_tokens as u32,
                            total_tokens: (usage.input_tokens + usage.output_tokens) as u32,
                            duration_ms: self.last_provider_call_ms,
                        }),
                        context_refs: step.required_context.clone(),
                        completion_time_ms: self.total_provider_duration_ms,
                        context_hash: String::new(),
                    };
                    step.evidence = Some(evidence);

                    steps_completed += 1;
                    completed_step_ids.push(step.step_id.clone());
                    if let Some(ref ev) = step.evidence {
                        completed_step_evidences.push(ev.clone());
                    }
                    final_text = text.clone();
                    step.status = StepStatus::Completed {
                        evidence: vec!["step completed with evidence".to_string()],
                    };
                    // Checkpoint: step completed
                    let _step_completed_checkpoint = AgentCheckpoint {
                        checkpoint_id: format!("{}-{}-completed", plan.plan_id, step.step_id),
                        task_id: plan.task_id.clone(),
                        plan_id: plan.plan_id.clone(),
                        step_id: step.step_id.clone(),
                        phase: CheckpointPhase::StepCompleted,
                        input_hash: String::new(),
                        context_hash: String::new(),
                        artifact_refs: vec![final_text.clone()],
                        tool_effects: vec![],
                        provider_usage: self.last_usage.as_ref().map(|usage| ProviderUsageSummary {
                            model: self
                                .provider
                                .models()
                                .into_iter()
                                .next()
                                .unwrap_or_else(|| "unknown".to_string()),
                            prompt_tokens: usage.input_tokens as u32,
                            completion_tokens: usage.output_tokens as u32,
                            total_tokens: (usage.input_tokens + usage.output_tokens) as u32,
                            duration_ms: self.last_provider_call_ms,
                        }),
                        budget_spent: 0,
                        approval_refs: vec![],
                        resume_policy: ResumePolicy::Skip,
                    };

                    self.emit(AgentLoopEvent::StepCompleted {
                        step_id: step.step_id.clone(),
                        evidence: vec!["step completed with evidence".to_string()],
                    });
                }
                Err(e) => {
                    let input_summary = vec![format!("step: {}", step.goal)];
                    let recovery = format!("{:?}", step.on_failure);

                    // Checkpoint: step failed
                    let failure_policy = match &step.on_failure {
                        StepFailureAction::Skip => ResumePolicy::Skip,
                        StepFailureAction::Retry { .. } => ResumePolicy::Rerun,
                        StepFailureAction::PauseForApproval => ResumePolicy::RequireApproval,
                        StepFailureAction::Abort => ResumePolicy::Abort,
                        _ => ResumePolicy::Abort,
                    };
                    let _step_failed_checkpoint = AgentCheckpoint {
                        checkpoint_id: format!("{}-{}-failed", plan.plan_id, step.step_id),
                        task_id: plan.task_id.clone(),
                        plan_id: plan.plan_id.clone(),
                        step_id: step.step_id.clone(),
                        phase: CheckpointPhase::StepStarted,
                        input_hash: String::new(),
                        context_hash: String::new(),
                        artifact_refs: vec![],
                        tool_effects: vec![],
                        provider_usage: None,
                        budget_spent: 0,
                        approval_refs: vec![],
                        resume_policy: failure_policy,
                    };

                    match &step.on_failure {
                        StepFailureAction::Skip => {
                            steps_skipped += 1;
                            step.status = StepStatus::Skipped { reason: e.clone() };
                            self.emit(AgentLoopEvent::StepFailed {
                                step_id: step.step_id.clone(),
                                reason: e.clone(),
                                action: "skip".into(),
                                input_context_summary: input_summary,
                                recovery_suggestion: recovery,
                            });
                            continue;
                        }
                        StepFailureAction::Stop | StepFailureAction::Abort => {
                            steps_failed += 1;
                            step.status = StepStatus::Failed {
                                reason: e.clone(),
                                input_context_summary: input_summary.clone(),
                                recovery_suggestion: recovery.clone(),
                            };
                            self.emit(AgentLoopEvent::StepFailed {
                                step_id: step.step_id.clone(),
                                reason: e.clone(),
                                action: "abort".into(),
                                input_context_summary: input_summary,
                                recovery_suggestion: recovery,
                            });
                            plan.status = PlanStatus::Failed {
                                failed_step: step.step_id.clone(),
                                reason: e.clone(),
                            };
                            self.emit(AgentLoopEvent::PlanCompleted {
                                plan_id: plan.plan_id.clone(),
                                steps_completed,
                                steps_failed,
                                steps_skipped,
                            });
                            let bundle = classify_failure(
                                &plan.plan_id,
                                &e,
                                &step.step_id,
                                &completed_step_ids,
                            );
                            self.emit(AgentLoopEvent::FailureBundle {
                                run_id: bundle.run_id,
                                failed_step: bundle.failed_step,
                                error_kind: bundle.error_kind,
                                completed_steps: bundle.completed_steps,
                                suggested_action: format!("{:?}", bundle.suggested_action),
                            });
                            self.emit_recovery_bundle(
                                &plan.plan_id,
                                step,
                                &e,
                                &completed_step_evidences,
                            );
                            self.emit_run_report(plan, &completed_step_evidences);
                            return Err(e);
                        }
                        StepFailureAction::RequestContextSupplement { sources } => {
                            steps_failed += 1;
                            step.status = StepStatus::Blocked {
                                reason: e.clone(),
                                context_summary: sources.clone(),
                                recovery_suggestion: format!(
                                    "Supplement context sources: {}",
                                    sources.join(", ")
                                ),
                            };
                            self.emit(AgentLoopEvent::StepBlocked {
                                step_id: step.step_id.clone(),
                                reason: e.clone(),
                                context_summary: sources.clone(),
                                recovery_suggestion: format!(
                                    "Supplement context sources: {}",
                                    sources.join(", ")
                                ),
                            });
                            plan.status = PlanStatus::Failed {
                                failed_step: step.step_id.clone(),
                                reason: e.clone(),
                            };
                            self.emit(AgentLoopEvent::PlanCompleted {
                                plan_id: plan.plan_id.clone(),
                                steps_completed,
                                steps_failed,
                                steps_skipped,
                            });
                            self.emit_run_report(plan, &completed_step_evidences);
                            return Err(e);
                        }
                        StepFailureAction::PauseForApproval => {
                            steps_failed += 1;
                            step.status = StepStatus::Blocked {
                                reason: e.clone(),
                                context_summary: vec!["Awaiting author approval".to_string()],
                                recovery_suggestion: "Resume after author approves this step."
                                    .to_string(),
                            };
                            self.emit(AgentLoopEvent::StepBlocked {
                                step_id: step.step_id.clone(),
                                reason: e.clone(),
                                context_summary: vec!["Awaiting author approval".to_string()],
                                recovery_suggestion: "Resume after author approves this step."
                                    .to_string(),
                            });
                            plan.status = PlanStatus::Failed {
                                failed_step: step.step_id.clone(),
                                reason: e.clone(),
                            };
                            self.emit(AgentLoopEvent::PlanCompleted {
                                plan_id: plan.plan_id.clone(),
                                steps_completed,
                                steps_failed,
                                steps_skipped,
                            });
                            return Err(e);
                        }
                        StepFailureAction::Retry { max_retries } => {
                            let mut retry_remaining = *max_retries;
                            loop {
                                let retry_result = self.run(user_message, true, true).await;
                                match retry_result {
                                    Ok(text) => {
                                        steps_completed += 1;
                                        completed_step_ids.push(step.step_id.clone());
                                        final_text = text;
                                        step.status = StepStatus::Completed { evidence: vec![] };
                                        self.emit(AgentLoopEvent::StepCompleted {
                                            step_id: step.step_id.clone(),
                                            evidence: vec![],
                                        });
                                        break;
                                    }
                                    Err(retry_error) => {
                                        if retry_remaining == 0 {
                                            steps_failed += 1;
                                            step.status = StepStatus::Failed {
                                                reason: retry_error.clone(),
                                                input_context_summary: input_summary.clone(),
                                                recovery_suggestion: recovery.clone(),
                                            };
                                            self.emit(AgentLoopEvent::StepFailed {
                                                step_id: step.step_id.clone(),
                                                reason: format!(
                                                    "Retry exhausted ({} attempts): {}",
                                                    max_retries, retry_error
                                                ),
                                                action: "abort".into(),
                                                input_context_summary: input_summary.clone(),
                                                recovery_suggestion: recovery.clone(),
                                            });
                                            plan.status = PlanStatus::Failed {
                                                failed_step: step.step_id.clone(),
                                                reason: retry_error.clone(),
                                            };
                                            self.emit(AgentLoopEvent::PlanCompleted {
                                                plan_id: plan.plan_id.clone(),
                                                steps_completed,
                                                steps_failed,
                                                steps_skipped,
                                            });
                                            let bundle = classify_failure(
                                                &plan.plan_id,
                                                &retry_error,
                                                &step.step_id,
                                                &completed_step_ids,
                                            );
                                            self.emit(AgentLoopEvent::FailureBundle {
                                                run_id: bundle.run_id,
                                                failed_step: bundle.failed_step,
                                                error_kind: bundle.error_kind,
                                                completed_steps: bundle.completed_steps,
                                                suggested_action: format!(
                                                    "{:?}",
                                                    bundle.suggested_action
                                                ),
                                            });
                                            self.emit_recovery_bundle(
                                                &plan.plan_id,
                                                step,
                                                &retry_error,
                                                &completed_step_evidences,
                                            );
                                            self.emit_run_report(plan, &completed_step_evidences);
                                            return Err(retry_error);
                                        }
                                        retry_remaining -= 1;
                                        tokio::time::sleep(std::time::Duration::from_millis(1000))
                                            .await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        plan.status = PlanStatus::Completed;
        self.emit(AgentLoopEvent::PlanCompleted {
            plan_id: plan.plan_id.clone(),
            steps_completed,
            steps_failed,
            steps_skipped,
        });

        // Build and emit AgentRunReport
        let runtime_calls = self
            .runtime_call_records
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let report = crate::run_trace::build_agent_run_report(
            plan.plan_id.clone(),
            plan,
            &runtime_calls,
            &completed_step_evidences,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        self.emit(AgentLoopEvent::RunReport { report });

        Ok(final_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{ExecutionStepState, PlanStatus, StepEvidence, StepFailureAction, StepStatus};
    use crate::provider::{LlmResponse, openai_compat::OpenAiCompatProvider};
    use crate::recovery::{classify_failure_kind, map_failure_to_recovery, FailureKind, RecoveryContext, RecoveryDecision};
    use crate::tool_registry::{
        default_writing_tool_registry, ToolDescriptor, ToolSideEffectLevel, ToolStage, ToolRegistry,
    };
    use async_trait::async_trait;

    /// Mock tool handler for testing.
    struct MockToolHandler;
    #[async_trait]
    impl ToolHandler for MockToolHandler {
        async fn execute(
            &self,
            tool_name: &str,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({
                "tool": tool_name,
                "args": args,
                "result": "mock"
            }))
        }
    }

    fn make_agent() -> AgentLoop<OpenAiCompatProvider, MockToolHandler> {
        make_agent_with_registry(default_writing_tool_registry())
    }

    fn make_agent_with_registry(
        registry: ToolRegistry,
    ) -> AgentLoop<OpenAiCompatProvider, MockToolHandler> {
        let provider = Arc::new(OpenAiCompatProvider::new(
            "https://api.openai.com/v1",
            "sk-test",
            "gpt-4o-mini",
        ));
        AgentLoop::new(
            AgentLoopConfig {
                max_rounds: 3,
                system_prompt: "You are a test agent.".into(),
                context_limit_tokens: None,
                tool_filter: None,
            },
            provider,
            registry,
            MockToolHandler,
        )
    }

    #[test]
    fn test_agent_creation() {
        let agent = make_agent();
        assert_eq!(agent.config.max_rounds, 3);
        assert!(agent.messages.is_empty());
    }

    #[test]
    fn test_add_user_message() {
        let mut agent = make_agent();
        agent.add_user_message("hello".into());
        assert_eq!(agent.messages.len(), 1);
        assert_eq!(agent.messages[0].role, "user");
        assert_eq!(agent.messages[0].content, Some("hello".into()));
    }

    #[test]
    fn test_estimate_tokens() {
        let mut agent = make_agent();
        agent.add_user_message("你好世界".repeat(50));
        let tokens = agent.estimate_tokens();
        // ~200 CJK chars / 3 ≈ 67 tokens + overhead + system prompt overhead
        assert!(tokens > 50);
    }

    #[test]
    fn test_build_tools_returns_valid_schema() {
        let agent = make_agent();
        let tools = agent.build_tools(&Intent::RetrieveKnowledge);
        // Tools without input_schema are filtered out; we check the return type
        for tool in &tools {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["name"].is_string());
        }
    }

    #[test]
    fn test_build_tools_hides_approval_required_write_tool() {
        let agent = make_agent();
        let tools = agent.build_tools(&Intent::GenerateContent);
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();

        assert!(!names.contains(&"generate_chapter_draft"));
    }

    #[test]
    fn test_build_tools_filters_schema_backed_approval_tool() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        });
        let mut registry = ToolRegistry::new();
        registry
            .register(
                ToolDescriptor::new(
                    "safe_preview",
                    "Safe preview.",
                    "none",
                    "json",
                    ToolSideEffectLevel::ProviderCall,
                    false,
                    100,
                    0,
                    ToolStage::Execute,
                )
                .with_supported_intents(&[Intent::GenerateContent])
                .with_input_schema(schema.clone()),
            )
            .unwrap();
        registry
            .register(
                ToolDescriptor::new(
                    "approval_write",
                    "Approval write.",
                    "none",
                    "json",
                    ToolSideEffectLevel::Write,
                    true,
                    100,
                    0,
                    ToolStage::Execute,
                )
                .with_supported_intents(&[Intent::GenerateContent])
                .with_input_schema(schema),
            )
            .unwrap();

        let agent = make_agent_with_registry(registry);
        let tools = agent.build_tools(&Intent::GenerateContent);
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();

        assert!(names.contains(&"safe_preview"));
        assert!(!names.contains(&"approval_write"));
    }

    #[test]
    fn test_build_tools_respects_task_filter_boundary() {
        let mut agent = make_agent();
        agent.config.tool_filter = Some(ToolFilter {
            intent: None,
            include_requires_approval: false,
            include_disabled: false,
            max_side_effect_level: Some(ToolSideEffectLevel::ProviderCall),
            required_tags: vec!["project".to_string()],
            allowed_names: Vec::new(),
        });

        let tools = agent.build_tools(&Intent::GenerateContent);
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();

        assert!(names.contains(&"load_current_chapter"));
        assert!(names.contains(&"load_outline_node"));
        assert!(names.contains(&"search_lorebook"));
        assert!(names.contains(&"query_project_brain"));
        assert!(!names.contains(&"generate_bounded_continuation"));
        assert!(!names.contains(&"generate_chapter_draft"));
    }

    #[test]
    fn test_event_callback() {
        let mut agent = make_agent();
        let emitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let emitted_clone = emitted.clone();
        agent.set_event_callback(Arc::new(move |ev| {
            emitted_clone.lock().unwrap().push(format!("{:?}", ev));
        }));
        agent.emit(AgentLoopEvent::Thinking);
        let events = emitted.lock().unwrap();
        assert!(!events.is_empty());
    }

    #[test]
    fn complete_event_serializes_ttft_fields() {
        let event = AgentLoopEvent::Complete {
            rounds: 3,
            tool_calls: 5,
            tokens_used: 12000,
            cached_tokens: Some(8000),
            input_tokens: Some(10000),
            output_tokens: Some(2000),
            ttft_ms: Some(320),
            total_provider_duration_ms: 4500,
            first_provider_call_ms: 1500,
            last_provider_call_ms: 3000,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "complete");
        assert_eq!(json["ttft_ms"], 320);
        assert_eq!(json["first_provider_call_ms"], 1500);
        assert_eq!(json["last_provider_call_ms"], 3000);
    }

    #[test]
    fn run_with_plan_events_serialize() {
        let event = AgentLoopEvent::PlanStarted {
            plan_id: "p1".into(),
            steps: vec!["step-0".into(), "step-1".into()],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "plan_started");
        assert_eq!(json["steps"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn step_failed_event_serializes_action() {
        let event = AgentLoopEvent::StepFailed {
            step_id: "step-1".into(),
            reason: "timeout".into(),
            action: "stop".into(),
            input_context_summary: vec!["step: draft".to_string()],
            recovery_suggestion: "retry".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["action"], "stop");
        assert!(json["input_context_summary"].is_array());
    }

    #[test]
    fn step_blocked_event_serializes() {
        let event = AgentLoopEvent::StepBlocked {
            step_id: "step-1".into(),
            reason: "awaiting approval".into(),
            context_summary: vec!["awaiting author approval".to_string()],
            recovery_suggestion: "Resume after approval".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "step_blocked");
        assert_eq!(json["reason"], "awaiting approval");
    }

    #[test]
    fn step_transition_event_serializes() {
        let event = AgentLoopEvent::StepTransition {
            step_id: "step-1".into(),
            from: "planned".into(),
            to: "ready".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "step_transition");
        assert_eq!(json["from"], "planned");
        assert_eq!(json["to"], "ready");
    }

    // ── A7: Agent Runtime Eval Harness tests ──

    /// Mock provider that can simulate various failure modes for testing.
    struct MockProvider {
        name: String,
        model: String,
        response_content: String,
        fail_with: Option<String>,
        fail_after_calls: std::sync::atomic::AtomicU32,
        call_count: std::sync::atomic::AtomicU32,
    }

    impl MockProvider {
        fn new(model: &str, response_content: &str) -> Self {
            Self {
                name: "mock-provider".to_string(),
                model: model.to_string(),
                response_content: response_content.to_string(),
                fail_with: None,
                fail_after_calls: std::sync::atomic::AtomicU32::new(0),
                call_count: std::sync::atomic::AtomicU32::new(0),
            }
        }

        fn with_failure(mut self, error: &str, after_calls: u32) -> Self {
            self.fail_with = Some(error.to_string());
            self.fail_after_calls = std::sync::atomic::AtomicU32::new(after_calls);
            self
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        fn models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }

        async fn stream_call(
            &self,
            _request: LlmRequest,
            on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
        ) -> Result<LlmResponse, String> {
            let count = self.call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(ref err) = self.fail_with {
                if count >= self.fail_after_calls.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(err.clone());
                }
            }
            on_event(StreamEvent::TextDelta {
                content: self.response_content.clone(),
            });
            on_event(StreamEvent::MessageStop {
                finish_reason: "stop".to_string(),
            });
            Ok(LlmResponse {
                content: Some(self.response_content.clone()),
                tool_calls: None,
                finish_reason: "stop".to_string(),
                usage: Some(crate::provider::UsageInfo {
                    input_tokens: 100,
                    output_tokens: 50,
                    cached_tokens: None,
                }),
            })
        }

        async fn call(&self, request: LlmRequest) -> Result<LlmResponse, String> {
            self.stream_call(request, Box::new(|_ev| {})).await
        }

        async fn embed(&self, _text: &str) -> Result<Vec<f32>, String> {
            Ok(vec![0.0; 128])
        }

        fn estimate_tokens(&self, messages: &[LlmMessage]) -> u64 {
            messages.iter().map(|m| m.content.as_ref().map(|c| c.chars().count()).unwrap_or(0) as u64 / 3 + 8).sum()
        }

        fn context_window_tokens(&self) -> u64 {
            128_000
        }

        async fn health_check(&self) -> Result<(), String> {
            Ok(())
        }
    }

    fn make_agent_with_mock_provider(
        provider: Arc<MockProvider>,
    ) -> AgentLoop<MockProvider, MockToolHandler> {
        AgentLoop::new(
            AgentLoopConfig {
                max_rounds: 3,
                system_prompt: "You are a test agent.".into(),
                context_limit_tokens: None,
                tool_filter: None,
            },
            provider,
            ToolRegistry::new(),
            MockToolHandler,
        )
    }

    #[tokio::test]
    async fn provider_budget_exceeded_triggers_block() {
        let provider = Arc::new(MockProvider::new("gpt-4", "ok"));
        let mut agent = make_agent_with_mock_provider(provider.clone());

        // Set up provider_call_guard that rejects calls due to budget
        agent.set_provider_call_guard(Arc::new(|_ctx| {
            Err("Provider budget exceeded: monthly ceiling reached".to_string())
        }));

        let mut plan = ExecutionPlan {
            plan_id: "plan-budget".to_string(),
            task_id: "task-1".to_string(),
            steps: vec![ExecutionStep {
                step_id: "step-0".to_string(),
                index: 0,
                goal: "draft chapter".to_string(),
                required_context: vec![],
                allowed_tools: vec![],
                max_side_effect: ToolSideEffectLevel::ProviderCall,
                success_signals: vec![],
                on_failure: StepFailureAction::Stop,
                status: StepStatus::Ready,
                step_state: ExecutionStepState::Ready,
                recovery_action: None,
                contract: None,
                evidence: None,
            }],
            status: PlanStatus::Pending,
            created_at_ms: 1,
        };

        let result = agent.run_with_plan(&mut plan, "write a chapter").await;
        assert!(result.is_err(), "expected provider budget to block the call");
        let err = result.unwrap_err();
        assert!(err.contains("budget") || err.contains("ceiling"), "expected budget error, got: {}", err);

        // Verify FailureKind and RecoveryDecision
        let kind = classify_failure_kind(&err, None);
        assert_eq!(kind, FailureKind::ProviderBudget);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::ShrinkContext);
    }

    #[tokio::test]
    async fn context_missing_preflight_blocks_provider_call() {
        let provider = Arc::new(MockProvider::new("gpt-4", "ok"));
        let mut agent = make_agent_with_mock_provider(provider.clone());

        // Set up provider_call_guard that rejects due to missing critical context
        agent.set_provider_call_guard(Arc::new(|_ctx| {
            Err("Critical context missing: outline not loaded".to_string())
        }));

        let mut plan = ExecutionPlan {
            plan_id: "plan-context".to_string(),
            task_id: "task-1".to_string(),
            steps: vec![ExecutionStep {
                step_id: "step-0".to_string(),
                index: 0,
                goal: "draft chapter".to_string(),
                required_context: vec!["outline".to_string()],
                allowed_tools: vec![],
                max_side_effect: ToolSideEffectLevel::ProviderCall,
                success_signals: vec![],
                on_failure: StepFailureAction::Stop,
                status: StepStatus::Ready,
                step_state: ExecutionStepState::Ready,
                recovery_action: None,
                contract: None,
                evidence: None,
            }],
            status: PlanStatus::Pending,
            created_at_ms: 1,
        };

        let result = agent.run_with_plan(&mut plan, "write a chapter").await;
        assert!(result.is_err(), "expected missing context to block provider call");
        let err = result.unwrap_err();
        assert!(err.contains("missing") || err.contains("outline"), "expected context missing error, got: {}", err);

        let kind = classify_failure_kind(&err, None);
        assert_eq!(kind, FailureKind::ContextMissing);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::CompactContext);
    }

    #[tokio::test]
    async fn provider_transient_failure_triggers_retry_with_backoff() {
        // Provider fails with 429 on first call, succeeds on retry
        let provider = Arc::new(
            MockProvider::new("gpt-4", "draft text")
                .with_failure("LLM call failed (429): rate limited", 0),
        );
        let mut agent = make_agent_with_mock_provider(provider.clone());
        agent.config.max_rounds = 1;

        let mut plan = ExecutionPlan {
            plan_id: "plan-retry".to_string(),
            task_id: "task-1".to_string(),
            steps: vec![ExecutionStep {
                step_id: "step-0".to_string(),
                index: 0,
                goal: "draft chapter".to_string(),
                required_context: vec![],
                allowed_tools: vec![],
                max_side_effect: ToolSideEffectLevel::ProviderCall,
                success_signals: vec![],
                on_failure: StepFailureAction::Retry { max_retries: 2 },
                status: StepStatus::Ready,
                step_state: ExecutionStepState::Ready,
                recovery_action: None,
                contract: None,
                evidence: None,
            }],
            status: PlanStatus::Pending,
            created_at_ms: 1,
        };

        // The run will fail because the mock provider always fails after the threshold
        let result = agent.run_with_plan(&mut plan, "write a chapter").await;
        assert!(result.is_err(), "expected transient failure");
        let err = result.unwrap_err();
        assert!(err.contains("429") || err.contains("rate limit"), "expected 429 error, got: {}", err);

        let kind = classify_failure_kind(&err, None);
        assert_eq!(kind, FailureKind::ProviderTransient);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::RetryWithBackoff);
    }

    #[tokio::test]
    async fn save_conflict_surfaces_user_choice() {
        let provider = Arc::new(MockProvider::new("gpt-4", "ok"));
        let mut agent = make_agent_with_mock_provider(provider.clone());

        // Simulate a save conflict by setting up a guard that rejects with conflict
        agent.set_provider_call_guard(Arc::new(|_ctx| {
            Err("Save conflict: revision mismatch".to_string())
        }));

        let mut plan = ExecutionPlan {
            plan_id: "plan-save".to_string(),
            task_id: "task-1".to_string(),
            steps: vec![ExecutionStep {
                step_id: "step-0".to_string(),
                index: 0,
                goal: "save chapter".to_string(),
                required_context: vec![],
                allowed_tools: vec![],
                max_side_effect: ToolSideEffectLevel::Write,
                success_signals: vec![],
                on_failure: StepFailureAction::Stop,
                status: StepStatus::Ready,
                step_state: ExecutionStepState::Ready,
                recovery_action: None,
                contract: None,
                evidence: None,
            }],
            status: PlanStatus::Pending,
            created_at_ms: 1,
        };

        let result = agent.run_with_plan(&mut plan, "save the chapter").await;
        assert!(result.is_err(), "expected save conflict to surface user choice");
        let err = result.unwrap_err();
        assert!(err.contains("conflict") || err.contains("mismatch"), "expected save conflict error, got: {}", err);

        // Verify SurfaceUserChoice decision (do NOT auto-overwrite)
        let kind = classify_failure_kind(&err, None);
        assert_eq!(kind, FailureKind::SaveConflict);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::SurfaceUserChoice);
    }

    #[tokio::test]
    async fn checkpoint_resume_skips_completed_steps() {
        let provider = Arc::new(MockProvider::new("gpt-4", "draft text"));
        let mut agent = make_agent_with_mock_provider(provider.clone());

        let mut plan = ExecutionPlan {
            plan_id: "plan-resume".to_string(),
            task_id: "task-1".to_string(),
            steps: vec![
                ExecutionStep {
                    step_id: "step-0".to_string(),
                    index: 0,
                    goal: "preflight".to_string(),
                    required_context: vec![],
                    allowed_tools: vec![],
                    max_side_effect: ToolSideEffectLevel::Read,
                    success_signals: vec![],
                    on_failure: StepFailureAction::Stop,
                    status: StepStatus::Completed { evidence: vec!["ok".to_string()] },
                    step_state: ExecutionStepState::Completed,
                    recovery_action: None,
                    contract: None,
                    evidence: Some(StepEvidence {
                        step_id: "step-0".to_string(),
                        artifact_refs: vec!["preflight-done".to_string()],
                        tool_executions: vec![],
                        provider_usage: None,
                        context_refs: vec![],
                        completion_time_ms: 500,
                        context_hash: "hash0".to_string(),
                    }),
                },
                ExecutionStep {
                    step_id: "step-1".to_string(),
                    index: 1,
                    goal: "draft".to_string(),
                    required_context: vec![],
                    allowed_tools: vec![],
                    max_side_effect: ToolSideEffectLevel::ProviderCall,
                    success_signals: vec![],
                    on_failure: StepFailureAction::Retry { max_retries: 1 },
                    status: StepStatus::Ready,
                    step_state: ExecutionStepState::Ready,
                    recovery_action: None,
                    contract: None,
                    evidence: None,
                },
            ],
            status: PlanStatus::Running,
            created_at_ms: 1,
        };

        // Resume the plan - step-0 is already completed, so it should be skipped
        let result = agent.resume_plan(&mut plan, "continue drafting").await;
        // The mock provider returns "draft text" which will succeed
        assert!(result.is_ok(), "expected resume to succeed for remaining steps");

        // Verify step-0 remained completed (was skipped)
        assert!(matches!(plan.steps[0].step_state, ExecutionStepState::Completed));
        assert!(matches!(&plan.steps[0].status, StepStatus::Completed { .. }));

        // Verify step-1 was executed and completed
        assert!(matches!(&plan.steps[1].status, StepStatus::Completed { .. }));
    }

    #[tokio::test]
    async fn failed_run_outputs_recovery_bundle() {
        let provider = Arc::new(MockProvider::new("gpt-4", "ok"));
        let mut agent = make_agent_with_mock_provider(provider.clone());

        // Set up a guard that always fails
        agent.set_provider_call_guard(Arc::new(|_ctx| {
            Err("Provider budget exceeded: monthly ceiling reached".to_string())
        }));

        let mut plan = ExecutionPlan {
            plan_id: "plan-bundle".to_string(),
            task_id: "task-1".to_string(),
            steps: vec![
                ExecutionStep {
                    step_id: "step-0".to_string(),
                    index: 0,
                    goal: "preflight".to_string(),
                    required_context: vec![],
                    allowed_tools: vec![],
                    max_side_effect: ToolSideEffectLevel::Read,
                    success_signals: vec![],
                    on_failure: StepFailureAction::Stop,
                    status: StepStatus::Completed { evidence: vec!["ok".to_string()] },
                    step_state: ExecutionStepState::Completed,
                    recovery_action: None,
                    contract: None,
                    evidence: Some(StepEvidence {
                        step_id: "step-0".to_string(),
                        artifact_refs: vec!["preflight-done".to_string()],
                        tool_executions: vec![],
                        provider_usage: None,
                        context_refs: vec![],
                        completion_time_ms: 500,
                        context_hash: "hash0".to_string(),
                    }),
                },
                ExecutionStep {
                    step_id: "step-1".to_string(),
                    index: 1,
                    goal: "draft".to_string(),
                    required_context: vec![],
                    allowed_tools: vec![],
                    max_side_effect: ToolSideEffectLevel::ProviderCall,
                    success_signals: vec![],
                    on_failure: StepFailureAction::Stop,
                    status: StepStatus::Ready,
                    step_state: ExecutionStepState::Ready,
                    recovery_action: None,
                    contract: None,
                    evidence: None,
                },
            ],
            status: PlanStatus::Running,
            created_at_ms: 1,
        };

        // Capture emitted events to verify RecoveryBundle
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_clone = events.clone();
        agent.set_event_callback(Arc::new(move |ev| {
            events_clone.lock().unwrap().push(ev);
        }));

        let result = agent.run_with_plan(&mut plan, "write a chapter").await;
        assert!(result.is_err(), "expected run to fail");

        // Verify that a RecoveryBundle event was emitted
        let emitted = events.lock().unwrap();
        let event_kinds: Vec<String> = emitted.iter().map(|e| {
            match e {
                AgentLoopEvent::PlanStarted { .. } => "PlanStarted".to_string(),
                AgentLoopEvent::StepStarted { .. } => "StepStarted".to_string(),
                AgentLoopEvent::StepFailed { .. } => "StepFailed".to_string(),
                AgentLoopEvent::StepCompleted { .. } => "StepCompleted".to_string(),
                AgentLoopEvent::PlanCompleted { .. } => "PlanCompleted".to_string(),
                AgentLoopEvent::FailureBundle { .. } => "FailureBundle".to_string(),
                AgentLoopEvent::RecoveryBundle { .. } => "RecoveryBundle".to_string(),
                AgentLoopEvent::Error { .. } => "Error".to_string(),
                _ => "Other".to_string(),
            }
        }).collect();
        eprintln!("Event kinds: {:?}", event_kinds);
        let recovery_bundle_events: Vec<_> = emitted
            .iter()
            .filter(|ev| matches!(ev, AgentLoopEvent::RecoveryBundle { .. }))
            .collect();
        assert!(
            !recovery_bundle_events.is_empty(),
            "expected at least one RecoveryBundle event to be emitted, got {:?}",
            event_kinds
        );

        // Verify the bundle contains correct information
        if let AgentLoopEvent::RecoveryBundle {
            completed_steps,
            failed_step,
            failure_kind,
            suggested_action,
            user_choice_required,
            ..
        } = recovery_bundle_events[0]
        {
            assert_eq!(failed_step, "step-1");
            assert_eq!(failure_kind, "provider_budget");
            assert_eq!(suggested_action, "shrink_context");
            assert!(!user_choice_required);
            assert_eq!(completed_steps.len(), 0);
            // completed_step_evidences is only populated from steps executed during this run
        } else {
            panic!("Expected RecoveryBundle event");
        }
    }
}
