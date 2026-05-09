pub async fn generate_chapter_draft(
    settings: &llm_runtime::LlmSettings,
    context: &BuiltChapterContext,
    provider_budget_approval: Option<&WriterProviderBudgetApproval>,
    mut ensure_provider_budget_allowed: impl FnMut(
            &BuiltChapterContext,
            &WriterProviderBudgetReport,
        ) -> Result<(), ChapterGenerationError>
        + Send,
    mut record_model_started: impl FnMut(&BuiltChapterContext, &WriterProviderBudgetReport) + Send,
) -> Result<GenerateChapterDraftOutput, ChapterGenerationError> {
    if context.prompt_context.trim().is_empty() {
        return Err(ChapterGenerationError::new(
            "CONTEXT_INVALID",
            "The built chapter context is empty.",
            true,
        ));
    }

    let system_prompt = format!(
        "You are a professional Chinese novelist drafting a complete chapter. \
Use the provided project context, preserve continuity, and write only chapter prose. \
Do not include analysis, markdown fences, action tags, or meta commentary. \
Preserve the named anchors, unresolved debts, and chapter mission constraints from the context; \
do not silently drop active named entities, artifacts, promises, or reader-debt payoffs unless the context says they are resolved. \
If the context names active anchors, carry the relevant anchors into the scene through action, dialogue, consequence, or payoff pressure; \
do not merely mention them in passing. \
Unless the chapter plan explicitly narrows scope, at least three active anchors from the context must materially participate in the scene, \
and at least one of them must change the immediate choice, pressure, or consequence of the chapter. \
Aim for {} Chinese characters, keep the output within {}-{} Chinese characters, and treat {}-{} as the hard save bounds unless the author explicitly overrides them.",
        context.chapter_contract.target_chars,
        context.chapter_contract.min_chars,
        context.chapter_contract.max_chars,
        context.chapter_contract.save_hard_floor_chars,
        context.chapter_contract.save_hard_ceiling_chars
    );

    // Append writing craft empowerment section
    let summary_snippet: String = context.prompt_context.chars().take(400).collect();
    let target_beat = context
        .sources
        .iter()
        .find(|s| s.source_type == "target_beat" || s.label.contains("beat"))
        .map(|s| s.label.as_str())
        .unwrap_or("");
    let open_promise_count = context
        .sources
        .iter()
        .filter(|s| s.source_type == "promise" || s.label.contains("promise"))
        .count();
    let has_near_payoff = context
        .sources
        .iter()
        .any(|s| s.source_type == "promise" && s.label.contains("near_payoff"));

    // Craft memory stats: if preflight cached stats in BuiltChapterContext,
    // the compiler adjusts rule priority by acceptance rate. Otherwise, default
    // priorities are used (no historical data yet).
    let craft_packet = compile_empowerment_prompt_with_memory(
        &summary_snippet,
        target_beat,
        open_promise_count,
        has_near_payoff,
        Some(5),
        Some(600),
        context.craft_rule_stats.as_ref(),
        &context.craft_memory_prompt_samples,
    );

    let system_prompt = if !craft_packet.chapter_discipline.is_empty() {
        let craft_section = format_craft_prompt_section(&craft_packet);
        format!("{}{}", system_prompt, craft_section)
    } else {
        system_prompt
    };

    let user_prompt = format!(
        "Task: {}\n\nTarget chapter: {}\n\nProject context:\n{}",
        context
            .sources
            .iter()
            .find(|s| s.source_type == "instruction")
            .map(|_| "Draft this chapter from the user's instruction.")
            .unwrap_or("Draft this chapter."),
        context.target.title,
        context.prompt_context
    );
    let messages = vec![
        serde_json::json!({"role": "system", "content": system_prompt}),
        serde_json::json!({"role": "user", "content": user_prompt}),
    ];
    let budget_report = apply_provider_budget_approval(
        chapter_generation_provider_budget(settings, &messages),
        provider_budget_approval,
    );
    if budget_report.decision == WriterProviderBudgetDecision::ApprovalRequired {
        return Err(provider_budget_error(
            &context.request_id,
            &context.receipt,
            budget_report,
        ));
    }
    ensure_provider_budget_allowed(context, &budget_report)?;
    record_model_started(context, &budget_report);

    let content = llm_runtime::chat_text_profile(
        settings,
        messages,
        llm_runtime::LlmRequestProfile::ChapterDraft,
        PROVIDER_TIMEOUT_SECS,
    )
    .await
    .map_err(map_provider_error)?;

    let content = content.trim().to_string();
    validate_generated_content_basics(&content)?;

    Ok(GenerateChapterDraftOutput {
        output_chars: char_count(&content),
        content,
        finish_reason: "complete".to_string(),
        model: settings.model.clone(),
        provider: "openai-compatible".to_string(),
        chapter_contract: context.chapter_contract.clone(),
        base_revision: context.base_revision.clone(),
        provider_budget: budget_report,
    })
}

pub async fn continue_chapter_draft(
    settings: &llm_runtime::LlmSettings,
    context: &BuiltChapterContext,
    existing_content: &str,
    provider_budget_approval: Option<&WriterProviderBudgetApproval>,
    mut ensure_provider_budget_allowed: impl FnMut(
            &BuiltChapterContext,
            &WriterProviderBudgetReport,
        ) -> Result<(), ChapterGenerationError>
        + Send,
    mut record_model_started: impl FnMut(&BuiltChapterContext, &WriterProviderBudgetReport) + Send,
) -> Result<ChapterDraftRepairOutput, ChapterGenerationError> {
    let existing_chars = char_count(existing_content);
    let deficit_to_min = context
        .chapter_contract
        .min_chars
        .saturating_sub(existing_chars);
    let room_to_max = context
        .chapter_contract
        .max_chars
        .saturating_sub(existing_chars);
    let add_min_chars = room_to_max.min((deficit_to_min + 80).max(260));
    let add_max_chars = room_to_max.min((deficit_to_min + 420).max(520));
    let target_floor = existing_chars + add_min_chars;
    let target_ceiling = existing_chars + add_max_chars;
    let system_prompt = format!(
        "You are a professional Chinese novelist continuing a chapter draft. \
Write only additional chapter prose that follows seamlessly from the existing draft. \
This is a bounded length repair, not a second chapter and not a new branch. \
Do not repeat earlier sentences, do not include analysis, and keep continuity stable. \
Add only {}-{} Chinese characters, stop as soon as the current action has a saveable consequence or hook, \
and keep the finished chapter within {}-{} Chinese characters. \
The finished chapter should move toward {} Chinese characters and must never exceed {} Chinese characters.",
        add_min_chars,
        add_max_chars,
        target_floor,
        target_ceiling,
        context.chapter_contract.target_chars,
        context.chapter_contract.max_chars
    );
    let user_prompt = format!(
        "Target chapter: {}\n\nExisting draft chars: {}\nRequired added chars: {}-{}\nFinal total target after this repair: {}-{}\n\nExisting draft:\n{}\n\nProject context:\n{}\n\nContinue with new prose only. Use 1-3 tight paragraphs to complete the unfinished action, consequence, or hook, then stop.",
        context.target.title,
        existing_chars,
        add_min_chars,
        add_max_chars,
        target_floor,
        target_ceiling,
        existing_content,
        context.prompt_context
    );
    let messages = vec![
        serde_json::json!({"role": "system", "content": system_prompt}),
        serde_json::json!({"role": "user", "content": user_prompt}),
    ];
    let budget_report = apply_provider_budget_approval(
        chapter_generation_provider_budget_for_profile(
            settings,
            &messages,
            llm_runtime::LlmRequestProfile::ChapterContinuation,
        ),
        provider_budget_approval,
    );
    if budget_report.decision == WriterProviderBudgetDecision::ApprovalRequired {
        return Err(provider_budget_error(
            &context.request_id,
            &context.receipt,
            budget_report,
        ));
    }
    ensure_provider_budget_allowed(context, &budget_report)?;
    record_model_started(context, &budget_report);

    let content = llm_runtime::chat_text_profile(
        settings,
        messages,
        llm_runtime::LlmRequestProfile::ChapterContinuation,
        PROVIDER_TIMEOUT_SECS,
    )
    .await
    .map(|text| text.trim().to_string())
    .map_err(map_provider_error)?;
    validate_generated_content_basics(&content)?;
    Ok(ChapterDraftRepairOutput {
        output_chars: char_count(&content),
        content,
        provider_budget: budget_report,
    })
}

pub async fn compress_chapter_draft(
    settings: &llm_runtime::LlmSettings,
    context: &BuiltChapterContext,
    existing_content: &str,
    provider_budget_approval: Option<&WriterProviderBudgetApproval>,
    ensure_provider_budget_allowed: impl FnMut(
            &BuiltChapterContext,
            &WriterProviderBudgetReport,
        ) -> Result<(), ChapterGenerationError>
        + Send,
    record_model_started: impl FnMut(&BuiltChapterContext, &WriterProviderBudgetReport) + Send,
) -> Result<ChapterDraftRepairOutput, ChapterGenerationError> {
    compress_chapter_draft_with_mode(
        settings,
        context,
        existing_content,
        provider_budget_approval,
        false,
        ensure_provider_budget_allowed,
        record_model_started,
    )
    .await
}

pub async fn compress_chapter_draft_hard(
    settings: &llm_runtime::LlmSettings,
    context: &BuiltChapterContext,
    existing_content: &str,
    provider_budget_approval: Option<&WriterProviderBudgetApproval>,
    ensure_provider_budget_allowed: impl FnMut(
            &BuiltChapterContext,
            &WriterProviderBudgetReport,
        ) -> Result<(), ChapterGenerationError>
        + Send,
    record_model_started: impl FnMut(&BuiltChapterContext, &WriterProviderBudgetReport) + Send,
) -> Result<ChapterDraftRepairOutput, ChapterGenerationError> {
    compress_chapter_draft_with_mode(
        settings,
        context,
        existing_content,
        provider_budget_approval,
        true,
        ensure_provider_budget_allowed,
        record_model_started,
    )
    .await
}

async fn compress_chapter_draft_with_mode(
    settings: &llm_runtime::LlmSettings,
    context: &BuiltChapterContext,
    existing_content: &str,
    provider_budget_approval: Option<&WriterProviderBudgetApproval>,
    hard_contract_repair: bool,
    mut ensure_provider_budget_allowed: impl FnMut(
            &BuiltChapterContext,
            &WriterProviderBudgetReport,
        ) -> Result<(), ChapterGenerationError>
        + Send,
    mut record_model_started: impl FnMut(&BuiltChapterContext, &WriterProviderBudgetReport) + Send,
) -> Result<ChapterDraftRepairOutput, ChapterGenerationError> {
    let existing_chars = char_count(existing_content);
    let target_floor = if hard_contract_repair {
        context
            .chapter_contract
            .min_chars
            .max(context.chapter_contract.target_chars.saturating_sub(500))
            .min(context.chapter_contract.max_chars)
    } else {
        context
            .chapter_contract
            .min_chars
            .max(context.chapter_contract.target_chars.saturating_sub(300))
            .min(context.chapter_contract.max_chars)
    };
    let target_ceiling = if hard_contract_repair {
        context.chapter_contract.max_chars.min(
            context
                .chapter_contract
                .target_chars
                .saturating_sub(100)
                .max(target_floor),
        )
    } else {
        context
            .chapter_contract
            .max_chars
            .min((context.chapter_contract.target_chars + 300).max(target_floor))
    };
    let required_cut = existing_chars.saturating_sub(target_ceiling);
    let repair_instruction = if hard_contract_repair {
        "This is a hard contract repair after a previous compression failed. \
Prioritize the character contract over preserving secondary detail. \
If detail and length conflict, delete side actions, explanatory background, repeated dialogue, and low-value internal monologue; \
preserve only the main causal chain, anchor participation, the character choice, and the ending hook."
    } else {
        "Preserve continuity, anchors, choices, consequences, and the ending hook while removing repeated description, side actions, filler dialogue, and redundant internal monologue."
    };
    let system_prompt = format!(
        "You are a professional Chinese novelist compressing a chapter draft. \
This is a deletion-and-tightening task, not a continuation. \
{} \
Write only the full revised chapter prose. Keep the chapter within {}-{} Chinese characters, and never exceed {} Chinese characters.",
        repair_instruction,
        target_floor,
        target_ceiling,
        context.chapter_contract.max_chars
    );
    let user_prompt = format!(
        "Target chapter: {}\n\nCurrent draft chars: {}\nMinimum cut required: {} chars\nTarget compressed range: {}-{}\nHard maximum: {}\n\nCurrent draft:\n{}\n\nProject context:\n{}\n\nRewrite as one complete, tighter chapter. Preserve the important anchors through action or consequence, but delete lower-value beats until the chapter fits the target range.",
        context.target.title,
        existing_chars,
        required_cut,
        target_floor,
        target_ceiling,
        context.chapter_contract.max_chars,
        existing_content,
        context.prompt_context
    );
    let messages = vec![
        serde_json::json!({"role": "system", "content": system_prompt}),
        serde_json::json!({"role": "user", "content": user_prompt}),
    ];
    let budget_report = apply_provider_budget_approval(
        chapter_generation_provider_budget_for_profile(
            settings,
            &messages,
            llm_runtime::LlmRequestProfile::ChapterCompress,
        ),
        provider_budget_approval,
    );
    if budget_report.decision == WriterProviderBudgetDecision::ApprovalRequired {
        return Err(provider_budget_error(
            &context.request_id,
            &context.receipt,
            budget_report,
        ));
    }
    ensure_provider_budget_allowed(context, &budget_report)?;
    record_model_started(context, &budget_report);

    let content = llm_runtime::chat_text_profile(
        settings,
        messages,
        llm_runtime::LlmRequestProfile::ChapterCompress,
        PROVIDER_TIMEOUT_SECS,
    )
    .await
    .map(|text| text.trim().to_string())
    .map_err(map_provider_error)?;
    validate_generated_content_basics(&content)?;
    Ok(ChapterDraftRepairOutput {
        output_chars: char_count(&content),
        content,
        provider_budget: budget_report,
    })
}

pub fn chapter_generation_provider_budget(
    settings: &llm_runtime::LlmSettings,
    messages: &[serde_json::Value],
) -> WriterProviderBudgetReport {
    chapter_generation_provider_budget_for_profile(
        settings,
        messages,
        llm_runtime::LlmRequestProfile::ChapterDraft,
    )
}

pub fn chapter_generation_provider_budget_for_profile(
    settings: &llm_runtime::LlmSettings,
    messages: &[serde_json::Value],
    profile: llm_runtime::LlmRequestProfile,
) -> WriterProviderBudgetReport {
    let converted = messages
        .iter()
        .map(|message| LlmMessage {
            role: message
                .get("role")
                .and_then(|value| value.as_str())
                .unwrap_or("user")
                .to_string(),
            content: message
                .get("content")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect::<Vec<_>>();
    let estimated_input_tokens =
        agent_harness_core::context_window_guard::estimate_request_tokens(&converted, None);
    evaluate_provider_budget(WriterProviderBudgetRequest::new(
        WriterProviderBudgetTask::ChapterGeneration,
        settings.model.clone(),
        estimated_input_tokens,
        u64::from(llm_runtime::request_options(settings, profile).max_tokens),
    ))
}

pub fn provider_budget_error(
    request_id: &str,
    receipt: &WriterTaskReceipt,
    report: WriterProviderBudgetReport,
) -> ChapterGenerationError {
    ChapterGenerationError::new(
        "PROVIDER_BUDGET_APPROVAL_REQUIRED",
        "Chapter generation provider budget requires explicit approval before calling the model.",
        true,
    )
    .with_evidence(Box::new(WriterFailureEvidenceBundle::new(
        WriterFailureCategory::ProviderFailed,
        "PROVIDER_BUDGET_APPROVAL_REQUIRED",
        "Chapter generation provider budget requires explicit approval before calling the model.",
        true,
        Some(request_id.to_string()),
        vec![
            format!("receipt:{}", receipt.task_id),
            format!("model:{}", report.model),
            format!("estimated_tokens:{}", report.estimated_total_tokens),
            format!("estimated_cost_micros:{}", report.estimated_cost_micros),
        ],
        serde_json::json!({
            "providerBudget": report,
            "receipt": receipt,
        }),
        vec![
            "Surface the provider token/cost estimate to the author before retrying.".to_string(),
            "Reduce context budget or requested output length if approval is not granted."
                .to_string(),
        ],
        crate::agent_runtime::now_ms(),
    )))
}

pub fn provider_budget_report_from_error(
    error: &ChapterGenerationError,
) -> Option<WriterProviderBudgetReport> {
    let budget = error
        .evidence
        .as_ref()?
        .details
        .get("providerBudget")?
        .clone();
    serde_json::from_value(budget).ok()
}

pub fn save_generated_chapter(
    project: &dyn ChapterGenerationProject,
    input: SaveGeneratedChapterInput,
) -> Result<SaveGeneratedChapterOutput, ChapterGenerationError> {
    if let Some(error) = validate_receipt_for_save(&input) {
        return Err(error);
    }

    if input.generated_content.trim().is_empty() {
        return Err(ChapterGenerationError::new(
            "CONTENT_EMPTY",
            "Generated chapter content is empty.",
            true,
        ));
    }

    if char_count(&input.generated_content) > DEFAULT_OUTPUT_HARD_CAP_CHARS {
        return Err(ChapterGenerationError::new(
            "CONTENT_TOO_LARGE",
            "Generated chapter content exceeds the hard save cap.",
            true,
        ));
    }
    validate_generated_content(
        &input.generated_content,
        &input.chapter_contract,
        ChapterContractPhase::Save,
    )?;

    let current_revision = project.chapter_revision(&input.target.title).map_err(|e| {
        ChapterGenerationError::with_details(
            "STORAGE_READ_FAILED",
            "Failed to read current chapter revision.",
            true,
            e,
        )
    })?;

    match decide_save_action(
        &input.target.title,
        &input.request_id,
        input.save_mode,
        &input.base_revision,
        &current_revision,
        input.frontend_state.as_ref(),
    ) {
        SaveDecision::WriteTarget => {
            let new_revision = project
                .save_chapter_content_and_revision(&input.target.title, &input.generated_content)
                .map_err(|e| {
                    ChapterGenerationError::with_details(
                        "STORAGE_WRITE_FAILED",
                        "Failed to save generated chapter.",
                        true,
                        e,
                    )
                })?;
            Ok(SaveGeneratedChapterOutput {
                chapter_title: input.target.title,
                new_revision,
                saved_mode: if current_revision == "missing" {
                    "created".to_string()
                } else {
                    "replaced".to_string()
                },
                output_chars: char_count(&input.generated_content),
            })
        }
        SaveDecision::WriteDraft {
            draft_title,
            conflict,
        } => {
            tracing::warn!(
                "Saving generated chapter as draft copy after conflict: {}",
                conflict.reason
            );
            let new_revision = project
                .save_chapter_content_and_revision(&draft_title, &input.generated_content)
                .map_err(|e| {
                    ChapterGenerationError::with_details(
                        "STORAGE_WRITE_FAILED",
                        "Failed to save generated draft copy.",
                        true,
                        e,
                    )
                })?;
            Ok(SaveGeneratedChapterOutput {
                chapter_title: draft_title,
                new_revision,
                saved_mode: "draft_copy".to_string(),
                output_chars: char_count(&input.generated_content),
            })
        }
        SaveDecision::Conflict(conflict) => Err(ChapterGenerationError {
            code: "SAVE_CONFLICT".to_string(),
            message: format!("Save blocked by {}.", conflict.reason),
            recoverable: true,
            details: serde_json::to_string(&conflict).ok(),
            evidence: Some(Box::new(failure_bundle_from_save_conflict(
                &input.receipt,
                &conflict,
                crate::agent_runtime::now_ms(),
            ))),
        }),
    }
}

fn validate_receipt_for_save(input: &SaveGeneratedChapterInput) -> Option<ChapterGenerationError> {
    let mismatches = input.receipt.validate_write_attempt(
        &input.request_id,
        &input.target.title,
        &input.base_revision,
        "saved_chapter",
    );
    if mismatches.is_empty() {
        return None;
    }

    let evidence_refs = mismatches
        .iter()
        .map(|mismatch| {
            format!(
                "{}:{}->{}",
                mismatch.field, mismatch.expected, mismatch.actual
            )
        })
        .collect::<Vec<_>>();
    let evidence = WriterFailureEvidenceBundle::new(
        WriterFailureCategory::ReceiptMismatch,
        "RECEIPT_MISMATCH",
        "Generated chapter save was blocked because the task receipt no longer matches the write attempt.",
        true,
        Some(input.receipt.task_id.clone()),
        evidence_refs,
        serde_json::json!({
            "receipt": input.receipt,
            "attempt": {
                "requestId": input.request_id,
                "chapter": input.target.title,
                "baseRevision": input.base_revision,
                "artifact": "saved_chapter",
            },
            "mismatches": mismatches,
        }),
        vec![
            "Rebuild the chapter generation context for the current target chapter.".to_string(),
            "Retry only after the frontend and storage revisions match.".to_string(),
        ],
        crate::agent_runtime::now_ms(),
    );

    Some(
        ChapterGenerationError::new(
            "RECEIPT_MISMATCH",
            "Generated chapter save was blocked because the task receipt no longer matches the write attempt.",
            true,
        )
        .with_evidence(Box::new(evidence)),
    )
}

pub fn save_conflict_from_error(error: &ChapterGenerationError) -> Option<SaveConflict> {
    if error.code != "SAVE_CONFLICT" {
        return None;
    }
    error
        .details
        .as_deref()
        .and_then(|details| serde_json::from_str(details).ok())
}

pub fn failure_bundle_from_chapter_error(
    request_id: &str,
    error: &ChapterGenerationError,
    created_at_ms: u64,
) -> WriterFailureEvidenceBundle {
    if let Some(bundle) = error.evidence.clone() {
        return *bundle;
    }
    let category = failure_category_for_error_code(&error.code);
    let mut evidence_refs = vec![format!("error:{}", error.code)];
    if let Some(details) = error
        .details
        .as_ref()
        .filter(|details| !details.trim().is_empty())
    {
        evidence_refs.push(format!("details:{}", snippet_text(details, 120)));
    }
    WriterFailureEvidenceBundle::new(
        category,
        error.code.clone(),
        error.message.clone(),
        error.recoverable,
        Some(request_id.to_string()),
        evidence_refs,
        serde_json::json!({
            "details": error.details,
        }),
        remediation_for_error_code(&error.code),
        created_at_ms,
    )
}

pub fn failure_bundle_from_save_conflict(
    receipt: &WriterTaskReceipt,
    conflict: &SaveConflict,
    created_at_ms: u64,
) -> WriterFailureEvidenceBundle {
    WriterFailureEvidenceBundle::new(
        WriterFailureCategory::SaveFailed,
        "SAVE_CONFLICT",
        format!("Save blocked by {}.", conflict.reason),
        true,
        Some(receipt.task_id.clone()),
        vec![
            format!("chapter:{}", receipt.chapter.clone().unwrap_or_default()),
            format!("base_revision:{}", conflict.base_revision),
            format!("current_revision:{}", conflict.current_revision),
            format!("save_conflict:{}", conflict.reason),
        ],
        serde_json::json!({
            "receipt": receipt,
            "conflict": conflict,
        }),
        vec![
            "Review the open editor changes before overwriting.".to_string(),
            "Regenerate from the current chapter revision or save as a draft copy.".to_string(),
        ],
        created_at_ms,
    )
}

fn failure_category_for_error_code(code: &str) -> WriterFailureCategory {
    match code {
        "INSTRUCTION_EMPTY"
        | "TARGET_CHAPTER_NOT_FOUND"
        | "TARGET_CHAPTER_AMBIGUOUS"
        | "CONTEXT_INVALID" => WriterFailureCategory::ContextMissing,
        "PROVIDER_TIMEOUT"
        | "PROVIDER_RATE_LIMITED"
        | "PROVIDER_NOT_CONFIGURED"
        | "PROVIDER_CALL_FAILED"
        | "PROVIDER_BUDGET_APPROVAL_REQUIRED"
        | "MODEL_OUTPUT_EMPTY"
        | "MODEL_OUTPUT_TOO_LARGE"
        | "MODEL_OUTPUT_UNDER_MIN_CHARS"
        | "MODEL_OUTPUT_OVER_MAX_CHARS" => WriterFailureCategory::ProviderFailed,
        "SAVE_CONFLICT"
        | "CONTENT_EMPTY"
        | "CONTENT_TOO_LARGE"
        | "CONTENT_UNDER_SAVE_FLOOR"
        | "CONTENT_OVER_SAVE_CEILING"
        | "STORAGE_READ_FAILED"
        | "STORAGE_WRITE_FAILED"
        | "OUTLINE_LOAD_FAILED"
        | "OUTLINE_SAVE_FAILED" => WriterFailureCategory::SaveFailed,
        "RECEIPT_MISMATCH" => WriterFailureCategory::ReceiptMismatch,
        _ => WriterFailureCategory::ProviderFailed,
    }
}

fn remediation_for_error_code(code: &str) -> Vec<String> {
    match code {
        "INSTRUCTION_EMPTY" => {
            vec!["Provide a concrete chapter generation instruction.".to_string()]
        }
        "TARGET_CHAPTER_NOT_FOUND" | "TARGET_CHAPTER_AMBIGUOUS" => {
            vec!["Select a concrete target chapter or fix duplicate outline entries.".to_string()]
        }
        "PROVIDER_NOT_CONFIGURED" => vec!["Configure a valid model provider API key.".to_string()],
        "PROVIDER_BUDGET_APPROVAL_REQUIRED" => vec![
            "Review and approve the estimated provider token/cost budget before retrying."
                .to_string(),
            "Reduce context budget or requested output length if approval is not granted."
                .to_string(),
        ],
        "PROVIDER_TIMEOUT" | "PROVIDER_RATE_LIMITED" | "PROVIDER_CALL_FAILED" => vec![
            "Retry after provider recovery or switch to another configured provider.".to_string(),
        ],
        "MODEL_OUTPUT_EMPTY"
        | "MODEL_OUTPUT_TOO_LARGE"
        | "MODEL_OUTPUT_UNDER_MIN_CHARS"
        | "MODEL_OUTPUT_OVER_MAX_CHARS" => vec![
            "Regenerate with a narrower chapter objective or smaller output budget.".to_string(),
        ],
        "CONTENT_UNDER_SAVE_FLOOR" | "CONTENT_OVER_SAVE_CEILING" => vec![
            "Review the chapter contract before saving generated content.".to_string(),
            "Regenerate, continue, or trim the chapter so it fits the save bounds.".to_string(),
        ],
        "RECEIPT_MISMATCH" => {
            vec!["Rebuild the task receipt from the latest context before saving.".to_string()]
        }
        "SAVE_CONFLICT" => {
            vec!["Resolve editor/storage revision mismatch or save as a draft copy.".to_string()]
        }
        _ => vec![
            "Inspect the failure evidence bundle and retry from the last safe phase.".to_string(),
        ],
    }
}

pub fn update_outline_after_generation(
    project: &dyn ChapterGenerationProject,
    target: &ChapterTarget,
    saved: &SaveGeneratedChapterOutput,
) -> Result<OutlineUpdateOutput, ChapterGenerationError> {
    let mut outline = project.load_outline().map_err(|e| {
        ChapterGenerationError::with_details(
            "OUTLINE_NOT_FOUND",
            "Failed to read outline for status update.",
            true,
            e,
        )
    })?;

    let mut changed = false;
    if let Some(node) = outline.iter_mut().find(|node| {
        node.chapter_title == saved.chapter_title || node.chapter_title == target.title
    }) {
        if node.status != "drafted" {
            node.status = "drafted".to_string();
            changed = true;
        }
    } else {
        outline.push(storage::OutlineNode {
            chapter_title: saved.chapter_title.clone(),
            summary: target.summary.clone(),
            status: "drafted".to_string(),
        });
        changed = true;
    }

    if changed {
        project.save_outline(&outline).map_err(|e| {
            ChapterGenerationError::with_details(
                "OUTLINE_UPDATE_FAILED",
                "Failed to update outline after chapter save.",
                true,
                e,
            )
        })?;
    }

    let outline_json = serde_json::to_string(&outline).unwrap_or_default();
    Ok(OutlineUpdateOutput {
        outline_revision: storage::content_revision(&outline_json),
        changed,
        warnings: vec![],
    })
}

pub struct ChapterGenerationConfig<P> {
    pub project: P,
    pub settings: llm_runtime::LlmSettings,
    pub payload: GenerateChapterAutonomousPayload,
    pub user_profile_entries: Vec<String>,
    pub project_id: String,
    pub memory_path: std::path::PathBuf,
}
