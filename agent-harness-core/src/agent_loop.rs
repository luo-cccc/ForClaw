use std::sync::Arc;

use crate::compaction::{compact_messages, should_compact, CompactionConfig};
use crate::context_window_guard::{
    evaluate_context_window, ContextWindowInfo, ContextWindowSource,
};
use crate::execution_plan::{ExecutionPlan, PlanStatus, StepFailureAction, StepStatus};
use crate::provider::{LlmMessage, LlmRequest, Provider, StreamEvent};
use crate::recovery::classify_failure;
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
    #[serde(rename = "step_failed")]
    StepFailed {
        step_id: String,
        reason: String,
        action: String,
    },
    #[serde(rename = "plan_completed")]
    PlanCompleted {
        plan_id: String,
        steps_completed: usize,
        steps_failed: usize,
    },
    #[serde(rename = "failure_bundle")]
    FailureBundle {
        run_id: String,
        failed_step: String,
        error_kind: String,
        completed_steps: Vec<String>,
        suggested_action: String,
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
        }
    }

    pub fn set_event_callback(&mut self, cb: EventCallback) {
        self.on_event = Some(cb);
    }

    pub fn set_provider_call_guard(&mut self, guard: ProviderCallGuard) {
        self.provider_call_guard = Some(guard);
    }

    fn emit(&self, event: AgentLoopEvent) {
        if let Some(ref cb) = self.on_event {
            cb(event);
        }
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
        let step_goals: Vec<String> = plan.steps.iter().map(|s| s.goal.clone()).collect();
        self.emit(AgentLoopEvent::PlanStarted {
            plan_id: plan.plan_id.clone(),
            steps: step_goals,
        });
        plan.status = PlanStatus::Running;

        let mut final_text = String::new();
        let mut steps_completed = 0usize;
        let mut steps_failed = 0usize;
        let mut completed_step_ids: Vec<String> = Vec::new();

        for step in &mut plan.steps {
            step.status = StepStatus::Active;
            self.emit(AgentLoopEvent::StepStarted {
                step_id: step.step_id.clone(),
                index: step.index,
                goal: step.goal.clone(),
            });

            // Constrain tool filter to this step's max_side_effect
            let saved_filter = self.config.tool_filter.clone();
            self.config.tool_filter = Some(crate::tool_registry::ToolFilter {
                intent: saved_filter.as_ref().and_then(|f| f.intent.clone()),
                include_requires_approval: false,
                include_disabled: false,
                max_side_effect_level: Some(step.max_side_effect),
                required_tags: Vec::new(),
            });

            let step_result = self.run(user_message, true, true).await;
            self.config.tool_filter = saved_filter;

            match step_result {
                Ok(text) => {
                    steps_completed += 1;
                    completed_step_ids.push(step.step_id.clone());
                    final_text = text;
                    step.status = StepStatus::Completed { evidence: vec![] };
                    self.emit(AgentLoopEvent::StepCompleted {
                        step_id: step.step_id.clone(),
                        evidence: vec![],
                    });
                }
                Err(e) => match step.on_failure {
                    StepFailureAction::Skip => {
                        steps_failed += 1;
                        step.status = StepStatus::Skipped { reason: e.clone() };
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: e.clone(),
                            action: "skip".into(),
                        });
                        continue;
                    }
                    StepFailureAction::Stop => {
                        steps_failed += 1;
                        step.status = StepStatus::Failed { reason: e.clone() };
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: e.clone(),
                            action: "stop".into(),
                        });
                        plan.status = PlanStatus::Failed {
                            failed_step: step.step_id.clone(),
                            reason: e.clone(),
                        };
                        self.emit(AgentLoopEvent::PlanCompleted {
                            plan_id: plan.plan_id.clone(),
                            steps_completed,
                            steps_failed,
                        });
                        // Emit failure bundle before returning error
                        let bundle =
                            classify_failure(&plan.plan_id, &e, &step.step_id, &completed_step_ids);
                        self.emit(AgentLoopEvent::FailureBundle {
                            run_id: bundle.run_id,
                            failed_step: bundle.failed_step,
                            error_kind: bundle.error_kind,
                            completed_steps: bundle.completed_steps,
                            suggested_action: format!("{:?}", bundle.suggested_action),
                        });
                        return Err(e);
                    }
                    StepFailureAction::Retry { max_retries: _ } => {
                        let retry_result = self.run(user_message, true, true).await;
                        if let Ok(text) = retry_result {
                            steps_completed += 1;
                            completed_step_ids.push(step.step_id.clone());
                            final_text = text;
                            step.status = StepStatus::Completed { evidence: vec![] };
                            self.emit(AgentLoopEvent::StepCompleted {
                                step_id: step.step_id.clone(),
                                evidence: vec![],
                            });
                            continue;
                        }
                        steps_failed += 1;
                        step.status = StepStatus::Failed { reason: e.clone() };
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: format!("Retry exhausted: {}", e),
                            action: "stop".into(),
                        });
                        plan.status = PlanStatus::Failed {
                            failed_step: step.step_id.clone(),
                            reason: e.clone(),
                        };
                        self.emit(AgentLoopEvent::PlanCompleted {
                            plan_id: plan.plan_id.clone(),
                            steps_completed,
                            steps_failed,
                        });
                        // Emit failure bundle before returning error
                        let bundle =
                            classify_failure(&plan.plan_id, &e, &step.step_id, &completed_step_ids);
                        self.emit(AgentLoopEvent::FailureBundle {
                            run_id: bundle.run_id,
                            failed_step: bundle.failed_step,
                            error_kind: bundle.error_kind,
                            completed_steps: bundle.completed_steps,
                            suggested_action: format!("{:?}", bundle.suggested_action),
                        });
                        return Err(e);
                    }
                },
            }
        }

        plan.status = PlanStatus::Completed;
        self.emit(AgentLoopEvent::PlanCompleted {
            plan_id: plan.plan_id.clone(),
            steps_completed,
            steps_failed,
        });

        Ok(final_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::openai_compat::OpenAiCompatProvider;
    use crate::tool_registry::{
        default_writing_tool_registry, ToolDescriptor, ToolSideEffectLevel, ToolStage,
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
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["action"], "stop");
    }
}
