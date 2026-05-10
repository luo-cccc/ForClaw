pub fn build_writing_checklist(memory: &crate::writer_agent::memory::WriterMemory, _chapter_title: &str) -> Vec<String> {
    let mut items = Vec::new();
    if let Ok(promises) = memory.get_open_promise_summaries() {
        for p in promises.iter().filter(|p| p.priority >= 5).take(3) {
            items.push(format!("兑现或推进线索: {}", p.title));
        }
    }
    if let Ok(chars) = memory.list_characters(Some("protagonist")) {
        for c in chars.iter().take(2) {
            items.push(format!("推进角色弧线: {}", c.name));
        }
    }
    if items.is_empty() {
        items.push("推进主线剧情".to_string());
    }
    items
}

pub fn character_voice_cards(memory: &crate::writer_agent::memory::WriterMemory) -> String {
    if let Ok(chars) = memory.list_characters(None) {
        let cards: Vec<String> = chars
            .iter()
            .take(5)
            .map(|c| format!("{} ({}): {}", c.name, c.role_type, c.current_state_summary))
            .collect();
        if cards.is_empty() {
            return String::new();
        }
        return format!("## 角色速写\n{}", cards.join("\n"));
    }
    String::new()
}

pub fn emotional_arc_guidance(memory: &crate::writer_agent::memory::WriterMemory, project_id: &str) -> String {
    let results = memory.list_recent_chapter_results(project_id, 1).unwrap_or_default();
    if let Some(latest) = results.first() {
        if !latest.summary.is_empty() {
            let snippet: String = latest.summary.chars().take(200).collect();
            return format!(
                "## 情感指引\n上一章读者感受: {}。请延续并回应读者的情感预期。",
                snippet
            );
        }
    }
    String::new()
}

pub fn author_voice_sample(memory: &crate::writer_agent::memory::WriterMemory, project_id: &str) -> String {
    let results = memory.list_recent_chapter_results(project_id, 1).unwrap_or_default();
    if let Some(latest) = results.first() {
        if !latest.summary.is_empty() {
            return format!(
                "## 参考你的写作风格\n{}",
                latest.summary.chars().take(300).collect::<String>()
            );
        }
    }
    String::new()
}

pub fn curated_context_summary(memory: &crate::writer_agent::memory::WriterMemory) -> String {
    let mut lines = Vec::new();
    if let Ok(promises) = memory.get_open_promise_summaries() {
        let mut sorted = promises.clone();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.priority));
        for p in sorted.iter().take(3) {
            lines.push(format!("线索: {} → {}", p.title, p.expected_payoff));
        }
    }
    if let Ok(items) = memory.list_knowledge_items(None) {
        for item in items.iter().take(3) {
            lines.push(format!("背景: {}", item.topic));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("## 关键信息\n{}", lines.join("\n"))
}

pub async fn build_chapter_context(
    project: &dyn ChapterGenerationProject,
    input: BuildChapterContextInput,
) -> Result<BuiltChapterContext, ChapterGenerationError> {
    let instruction = input.user_instruction.trim();
    if instruction.is_empty() {
        return Err(ChapterGenerationError::new(
            "INSTRUCTION_EMPTY",
            "The chapter generation instruction is empty.",
            true,
        ));
    }

    // P9: Open memory once and reuse across all read-only queries.
    let memory = crate::writer_agent::memory::WriterMemory::open(project.memory_path()).ok();

    let outline = project.load_outline().map_err(|e| {
        ChapterGenerationError::with_details(
            "STORAGE_READ_FAILED",
            "Failed to read outline.",
            true,
            e,
        )
    })?;

    let target = resolve_target_from_outline(
        &outline,
        input.target_chapter_title.as_deref(),
        input.target_chapter_number,
        input.chapter_summary_override.as_deref(),
    )?;

    // Parallelize independent read-only I/O: chapter_revision + lorebook
    let target_title = target.title.clone();
    let rev_project = project.box_clone();
    let rev_future = tokio::task::spawn_blocking(move || {
        rev_project.chapter_revision(&target_title).map_err(|e| {
            ChapterGenerationError::with_details(
                "STORAGE_READ_FAILED",
                "Failed to read target chapter revision.",
                true,
                e,
            )
        })
    });
    let lore_project = project.box_clone();
    let lore_future = tokio::task::spawn_blocking(move || {
        lore_project.load_lorebook().map_err(|e| {
            ChapterGenerationError::with_details(
                "STORAGE_READ_FAILED",
                "Failed to read lorebook.",
                true,
                e,
            )
        })
    });
    let (base_revision_result, lorebook_result) =
        futures_util::try_join!(rev_future, lore_future).map_err(|e| {
            ChapterGenerationError::with_details(
                "JOIN_ERROR",
                format!("Parallel read failed: {}", e),
                true,
                e.to_string(),
            )
        })?;
    let base_revision = base_revision_result?;
    let lore_entries = lorebook_result?;

    let chapter_contract = input.chapter_contract.validate()?;
    let query = format!("{}\n{}\n{}", instruction, target.title, target.summary);
    let mut composer = ContextComposer::new(input.budget.total_chars);
    composer.add_source_with_meta(
        "instruction",
        "user-instruction",
        "User instruction",
        instruction,
        input.budget.instruction_chars,
        None,
        0,
        "ok",
        agent_harness_core::TAXONOMY_INSTRUCTION,
        "directive",
    );

    let outline_text = if outline.is_empty() {
        "No outline nodes found.".to_string()
    } else {
        outline
            .iter()
            .enumerate()
            .map(|(idx, node)| {
                format!(
                    "{}. {} [{}]\n{}",
                    idx + 1,
                    node.chapter_title,
                    node.status,
                    node.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    composer.add_source_with_meta(
        "outline",
        "outline.json",
        "Outline / beat sheet",
        &outline_text,
        input.budget.outline_chars,
        None,
        0,
        "ok",
        agent_harness_core::TAXONOMY_OUTLINE,
        "grounding",
    );

    composer.add_source_with_meta(
        "target_beat",
        &target.title,
        "Current chapter beat",
        &build_target_beat_context(&target.summary),
        input.budget.outline_chars.min(2_000),
        None,
        0,
        "ok",
        agent_harness_core::TAXONOMY_SCENE_PLAN,
        "grounding",
    );

    let mut previous_fulltext_upgrade_count: usize = 0;
    let mut previous_fulltext_upgrade_reason = String::new();

    if let Some(target_index) = target.number.map(|n| n - 1) {
        let prev_t0 = std::time::Instant::now();
        let previous_nodes =
            select_previous_nodes(&outline, target_index, input.budget.previous_chapter_count);
        let mut previous_text = build_adjacent_chapter_context(project, previous_nodes.clone());

        // Risk gate: upgrade to fulltext when continuity risk is elevated.
        let open_promise_count = input.open_promise_count;
        let unresolved_debt_density = open_promise_count;
        let continuity_risk = if open_promise_count > 5 {
            "high"
        } else if open_promise_count > 2 {
            "medium"
        } else {
            "low"
        };
        let previous_structured_evidence_insufficient = previous_text.len() < 100;

        let should_upgrade_fulltext = continuity_risk == "high"
            || unresolved_debt_density > 3
            || previous_structured_evidence_insufficient;

        if should_upgrade_fulltext {
            let mut reasons = Vec::new();
            if continuity_risk == "high" {
                reasons.push(format!(
                    "continuity_risk=high (open_promises={})",
                    open_promise_count
                ));
            }
            if unresolved_debt_density > 3 {
                reasons.push(format!(
                    "unresolved_debt_density={}",
                    unresolved_debt_density
                ));
            }
            if previous_structured_evidence_insufficient {
                reasons.push("structured_evidence_insufficient".to_string());
            }
            previous_fulltext_upgrade_reason = reasons.join("; ");

            let fulltext_futures: Vec<_> = previous_nodes
                .iter()
                .map(|node| {
                    let title = node.chapter_title.clone();
                    let proj = project.box_clone();
                    tokio::task::spawn_blocking(move || {
                        proj.load_chapter(&title).ok().filter(|f| !f.trim().is_empty())
                    })
                })
                .collect();
            for (node, handle) in previous_nodes.iter().zip(fulltext_futures) {
                if let Ok(Some(full)) = handle.await {
                    let snippet = snippet_text(&full, 1200);
                    previous_text.push_str(&format!(
                        "\n\n## Previous chapter fulltext: {} (risk upgrade)\n{}",
                        node.chapter_title, snippet
                    ));
                    previous_fulltext_upgrade_count += 1;
                }
            }
        }
        let prev_elapsed_ms = prev_t0.elapsed().as_millis() as u64;

        let prev_status = if previous_text.trim().is_empty() {
            "not_found"
        } else {
            "ok"
        };
        composer.add_source_with_meta(
            "previous_chapters",
            "previous",
            "Previous chapter continuity",
            &previous_text,
            input.budget.previous_chapters_chars,
            None,
            prev_elapsed_ms,
            prev_status,
            agent_harness_core::TAXONOMY_PRIOR_CHAPTER,
            "continuity",
        );

        let next_t0 = std::time::Instant::now();
        let next_nodes = select_next_nodes(&outline, target_index, input.budget.next_chapter_count);
        let next_text = build_next_chapter_context(next_nodes);
        let next_elapsed_ms = next_t0.elapsed().as_millis() as u64;
        let next_status = if next_text.trim().is_empty() {
            "not_found"
        } else {
            "ok"
        };
        composer.add_source_with_meta(
            "next_chapter",
            "next",
            "Next chapter direction",
            &next_text,
            input.budget.next_chapter_chars,
            None,
            next_elapsed_ms,
            next_status,
            agent_harness_core::TAXONOMY_SCENE_PLAN,
            "foreshadowing",
        );
    }

    // Parallelize target existing text + RAG chunks
    let rag_t0 = std::time::Instant::now();
    let target_title_existing = target.title.clone();
    let existing_project = project.box_clone();
    let existing_future = tokio::task::spawn_blocking(move || {
        existing_project.load_chapter(&target_title_existing).ok().filter(|f| !f.trim().is_empty())
    });
    let query_rag = query.clone();
    let rag_project = project.box_clone();
    let rag_future = tokio::task::spawn_blocking(move || {
        select_rag_chunks(&*rag_project,
            &query_rag,
            input.budget.rag_chunk_count,
        )
    });
    let (existing_opt, rag_chunks_result) = futures_util::join!(existing_future, rag_future);
    let rag_chunks = rag_chunks_result.unwrap_or_default();
    let existing_elapsed_ms = if let Ok(Some(_)) = &existing_opt {
        // timing is approximate since we join’d with RAG; use a small fixed estimate
        1
    } else {
        0
    };
    if let Ok(Some(existing)) = existing_opt {
        let existing_status = if existing.trim().is_empty() { "not_found" } else { "ok" };
        composer.add_source_with_meta(
            "target_existing_text",
            &target.title,
            "Existing target chapter text",
            &existing,
            input.budget.target_existing_chars,
            None,
            existing_elapsed_ms,
            existing_status,
            agent_harness_core::TAXONOMY_PRIOR_CHAPTER,
            "continuity",
        );
    }

    // Read-only sources: lorebook pre-fetched in parallel with chapter_revision above
    let lore_t0 = std::time::Instant::now();
    let selected_lore =
        select_lore_entries(&lore_entries, &query, input.budget.lorebook_entry_count);
    let lore_text = if selected_lore.is_empty() {
        "No directly relevant lorebook entries found.".to_string()
    } else {
        selected_lore
            .iter()
            .map(|(score, entry)| {
                format!("[{}] score {:.1}\n{}", entry.keyword, score, entry.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let lore_elapsed_ms = lore_t0.elapsed().as_millis() as u64;
    let lore_status = if lore_text.contains("No directly relevant lorebook entries") {
        "not_found"
    } else {
        "ok"
    };
    composer.add_source_with_meta(
        "lorebook",
        "lorebook.json",
        "Relevant lorebook entries",
        &lore_text,
        input.budget.lorebook_chars,
        None,
        lore_elapsed_ms,
        lore_status,
        agent_harness_core::TAXONOMY_LORE,
        "grounding",
    );

    let rag_elapsed_ms = rag_t0.elapsed().as_millis() as u64;
    if !rag_chunks.is_empty() {
        let rag_text = rag_chunks
            .iter()
            .map(|(score, reasons, chunk)| {
                format!(
                    "[{} · {} · score {:.1}]\n{}\n{}",
                    chunk.id,
                    chunk.chapter,
                    score,
                    format_text_chunk_relevance(reasons),
                    chunk.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        composer.add_source_with_meta(
            "project_brain",
            "project_brain.json",
            "Project Brain relevant chunks",
            &rag_text,
            input.budget.rag_chars,
            Some(
                rag_chunks
                    .first()
                    .map(|(score, _, _)| *score)
                    .unwrap_or_default(),
            ),
            rag_elapsed_ms,
            "ok",
            agent_harness_core::TAXONOMY_PROJECT_BRAIN,
            "memory",
        );
    } else {
        // RAG returned no chunks; source is absent from composer but retrieval was attempted
        composer.add_source_with_meta(
            "project_brain",
            "project_brain.json",
            "Project Brain relevant chunks",
            "",
            input.budget.rag_chars,
            None,
            rag_elapsed_ms,
            "not_found",
            agent_harness_core::TAXONOMY_PROJECT_BRAIN,
            "memory",
        );
    }

    let profile_t0 = std::time::Instant::now();
    let profile_text = input
        .user_profile_entries
        .iter()
        .take(input.budget.user_profile_entry_count)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let profile_elapsed_ms = profile_t0.elapsed().as_millis() as u64;
    if !profile_text.trim().is_empty() {
        composer.add_source_with_meta(
            "user_profile",
            "user_drift_profile",
            "User style preferences",
            &profile_text,
            input.budget.user_profile_chars,
            None,
            profile_elapsed_ms,
            "ok",
            agent_harness_core::TAXONOMY_AUTHOR_VOICE,
            "style",
        );
    }

    let (mut prompt_context, sources, budget_report) = composer.finish();
    let warnings = budget_report.warnings.clone();
    let quality_anchor_keywords =
        build_quality_anchor_keywords(&target, &selected_lore, &sources, input.compiled_input.as_ref());
    let author_voice_snapshot = memory
        .as_ref()
        .and_then(|mem| build_quality_author_voice_snapshot(mem, project.project_id()));
    let required_story_anchors =
        build_required_story_anchors(project, memory.as_ref(), &target, &selected_lore);

    if let Some(ref ci) = input.compiled_input {
        let evidence_text = ci.selected_evidence.join("\n");
        let rules_text = ci.rule_stack.join("\n");
        let block = format!(
            "\n## 本章生成计划\n意图: {}\n证据: {}\n规则: {}\n",
            ci.intent_text, evidence_text, rules_text
        );
        prompt_context.push_str(&block);
    }
    // Attempt story impact scoping: check whether we can drop non-impacted
    // evidence sources before building the final evidence artifact.
    let (impact_scoped, impact_filtered_count) = {
        if let Some(ref mem) = memory {
            let has_impact = mem.get_open_promises().ok().map(|p| !p.is_empty()).unwrap_or(false)
                || mem
                    .list_characters(None)
                    .ok()
                    .map(|chars| !chars.is_empty())
                    .unwrap_or(false);
            if has_impact {
                let impacted_types: std::collections::HashSet<&str> = [
                    "instruction",
                    "outline",
                    "target_beat",
                    "previous_chapters",
                    "lorebook",
                    "project_brain",
                ]
                .into_iter()
                .collect();
                let filtered = sources
                    .iter()
                    .filter(|s| impacted_types.contains(s.source_type.as_str()))
                    .count();
                (true, filtered)
            } else {
                (false, 0)
            }
        } else {
            (false, 0)
        }
    };

    // Writing quality enrichment: enrich the chapter prompt with checklist and context.
    {
        if let Some(ref memory) = memory {
            let checklist = build_writing_checklist(memory, &target.title);
            let checklist_str = checklist
                .iter()
                .map(|s| format!("- {}", s))
                .collect::<Vec<_>>()
                .join("\n");
            prompt_context = format!(
                "## 本章写作清单\n{}\n\n{}",
                checklist_str, prompt_context
            );
            let curated = curated_context_summary(memory);
            if !curated.is_empty() {
                prompt_context = format!("{}{}\n\n", prompt_context, curated);
            }
            let voice_cards = character_voice_cards(memory);
            if !voice_cards.is_empty() {
                prompt_context = format!("{}{}\n\n", prompt_context, voice_cards);
            }
            let voice = author_voice_sample(memory, project.project_id());
            if !voice.is_empty() {
                prompt_context = format!("{}{}\n\n", prompt_context, voice);
            }
            let arc_guidance = emotional_arc_guidance(memory, project.project_id());
            if !arc_guidance.is_empty() {
                prompt_context = format!("{}{}\n\n", prompt_context, arc_guidance);
            }
        }
    }

    let intent_artifact = build_chapter_intent_artifact(instruction, &target);
    let selected_evidence = build_selected_evidence_artifact(&sources);
    let rule_stack = build_chapter_rule_stack(&chapter_contract);
    let trace_artifact = ChapterTraceArtifact {
        chapter_number: target.number,
        planner_inputs: vec![
            "instruction".to_string(),
            "outline".to_string(),
            "target_beat".to_string(),
            "previous_chapters".to_string(),
            "lorebook".to_string(),
            "project_brain".to_string(),
        ],
        selected_evidence_count: selected_evidence.len(),
        active_override_count: if input.chapter_summary_override.as_deref().is_some_and(|s| !s.trim().is_empty()) {
            1
        } else {
            0
        },
    };

    let request_id = input.request_id;
    let receipt = build_chapter_generation_receipt(
        &request_id,
        &target,
        &base_revision,
        instruction,
        &sources,
        crate::agent_runtime::now_ms(),
    );

    let scene_plan = vec![ScenePlanEntry {
        name: target.title.clone(),
        objective: intent_artifact.goal.clone(),
        participants: Vec::new(),
    }];

    let stable_prefix_chars: usize = sources.iter().take(3).map(|s| s.included_chars).sum();
    let dynamic_tail_chars: usize = sources.iter().skip(3).map(|s| s.included_chars).sum();

    let mut rebuild_count: usize = 0;
    if let Ok(mut state) = focus_state().lock() {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        instruction.hash(&mut hasher);
        let result_hash = format!("{:x}", hasher.finish());
        let mut hasher2 = std::collections::hash_map::DefaultHasher::new();
        target.summary.hash(&mut hasher2);
        let next_beat_hash = format!("{:x}", hasher2.finish());
        let needs_rebuild = state.needs_rebuild(
            &target.title,
            None,
            &result_hash,
            &next_beat_hash,
        );
        if needs_rebuild {
            state.record_rebuild(&target.title, None, &result_hash, &next_beat_hash);
        }
        rebuild_count = state.rebuild_count;
    }

    let context_quality = {
        let required_types: Vec<String> = [
            "instruction",
            "outline",
            "target_beat",
            "previous_chapters",
            "lorebook",
            "project_brain",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let packed = agent_harness_core::PackedContext {
            text: prompt_context.clone(),
            sources: sources
                .iter()
                .map(|s| agent_harness_core::ContextSourceReport {
                    source_type: s.source_type.clone(),
                    id: s.id.clone(),
                    label: s.label.clone(),
                    original_chars: s.original_chars,
                    included_chars: s.included_chars,
                    truncated: s.truncated,
                    score: s.score,
                    taxonomy: s.taxonomy.clone(),
                    role: s.role.clone(),
                    elapsed_ms: s.elapsed_ms,
                    retrieval_status: s.retrieval_status.clone(),
                })
                .collect(),
            budget: agent_harness_core::ContextBudgetReport {
                max_chars: budget_report.max_chars,
                included_chars: budget_report.included_chars,
                source_count: budget_report.source_count,
                truncated_source_count: budget_report.truncated_source_count,
                warnings: budget_report.warnings.clone(),
            },
            context_hash: String::new(),
        };
        Some(agent_harness_core::evaluate_context_quality(
            &request_id,
            &packed,
            &required_types,
        ))
    };
    let mut warnings = warnings;
    if let Some(ref quality) = context_quality {
        match &quality.recommendation {
            agent_harness_core::ContextQualityRecommendation::Critical { reason } => {
                return Err(ChapterGenerationError::with_details(
                    "CONTEXT_QUALITY_CRITICAL",
                    "Chapter generation blocked because context quality is critically low.",
                    true,
                    reason.clone(),
                ));
            }
            agent_harness_core::ContextQualityRecommendation::Supplement { sources, .. } => {
                warnings.push(format!(
                    "Context quality suggests supplementing sources: {}",
                    if sources.is_empty() {
                        "review truncated or weak grounding sources".to_string()
                    } else {
                        sources.join(", ")
                    }
                ));
            }
            agent_harness_core::ContextQualityRecommendation::Sufficient => {}
        }
        warnings.extend(
            quality
                .warnings
                .iter()
                .map(|warning| format!("Context quality warning: {}", warning)),
        );
    }

    // Build SceneCraftPlan from intent and outline data
    let next_summary = target
        .number
        .and_then(|n| outline.get(n)) // n is 1-indexed, outline.get(n) is the next chapter
        .map(|node| node.summary.clone());
    let craft_plan = {
        let participants: Vec<String> = scene_plan
            .iter()
            .flat_map(|entry| entry.participants.clone())
            .collect();
        let packet = compile_empowerment_prompt(
            &intent_artifact.goal,
            &target.summary,
            input.open_promise_count,
            false,
            Some(5),
            Some(600),
            None,
        );
        Some(build_scene_craft_plan(
            &target.title,
            &intent_artifact.goal,
            &participants,
            &target.summary,
            next_summary.as_deref(),
            &[],
            &packet,
        ))
    };
    let required_state_deltas =
        build_required_state_deltas(&target, &intent_artifact, memory.as_ref(), next_summary.as_deref());

    Ok(BuiltChapterContext {
        request_id,
        target,
        base_revision,
        chapter_contract,
        prompt_context,
        sources,
        budget: budget_report,
        warnings,
        receipt,
        intent_artifact,
        selected_evidence,
        rule_stack,
        trace_artifact,
        scene_plan,
        craft_plan,
        compiled_input: input.compiled_input.clone(),
        stable_prefix_chars,
        dynamic_tail_chars,
        focus_pack_rebuild_count: rebuild_count,
        previous_fulltext_upgrade_count,
        previous_fulltext_upgrade_reason,
        impact_scoped,
        impact_filtered_count,
        impact_truncated: false,
        generation_strategy: GenerationStrategy::default(),
        context_quality,
        craft_rule_stats: memory.as_ref().map(|mem| {
            let conn = mem.connection();
            let mut stats = std::collections::HashMap::new();
            for rule in craft_library_for_stats() {
                if let Some(s) = crate::writer_agent::memory::get_craft_rule_stats(conn, &rule.id) {
                    stats.insert(rule.id.clone(), s);
                }
            }
            stats
        }),
        craft_memory_prompt_samples: memory
            .as_ref()
            .map(|mem| build_craft_memory_prompt_samples(mem.connection()))
            .unwrap_or_default(),
        quality_anchor_keywords,
        author_voice_snapshot,
        required_story_anchors,
        required_state_deltas,
    })
}

fn build_craft_memory_prompt_samples(
    conn: &rusqlite::Connection,
) -> Vec<CraftMemoryPromptSamples> {
    let mut samples = Vec::new();
    for rule in craft_library_for_stats() {
        let examples = crate::writer_agent::memory::list_craft_examples(conn, &rule.id, 2)
            .unwrap_or_default()
            .into_iter()
            .map(|example| CraftMemoryPromptExample {
                rule_id: example.rule_id,
                excerpt_ref: example.excerpt_ref,
                excerpt: example.excerpt,
                reason: example.reason,
                score_delta: example.score_delta,
            })
            .collect::<Vec<_>>();
        let bad_patterns = crate::writer_agent::memory::list_craft_bad_patterns(conn, &rule.id, 2)
            .unwrap_or_default()
            .into_iter()
            .map(|pattern| CraftMemoryPromptBadPattern {
                rule_id: pattern.rule_id,
                evidence_ref: pattern.evidence_ref,
                evidence_excerpt: pattern.evidence_excerpt,
                correction: pattern.correction,
                rejected_count: pattern.rejected_count,
            })
            .collect::<Vec<_>>();
        if !examples.is_empty() || !bad_patterns.is_empty() {
            samples.push(CraftMemoryPromptSamples {
                rule_id: rule.id.clone(),
                examples,
                bad_patterns,
            });
        }
    }
    samples
}

fn build_quality_anchor_keywords(
    target: &ChapterTarget,
    selected_lore: &[(f32, &storage::LoreEntry)],
    sources: &[ChapterContextSource],
    compiled_input: Option<&CompiledInput>,
) -> Vec<String> {
    let mut anchors = Vec::new();
    for (_, entry) in selected_lore.iter().take(8) {
        push_anchor_candidate(&mut anchors, &entry.keyword);
    }
    for source in sources.iter().filter(|source| {
        matches!(
            source.source_type.as_str(),
            "previous_chapters" | "lorebook" | "project_brain"
        )
    }) {
        push_anchor_candidate(&mut anchors, &source.id);
    }
    for token in extract_story_anchor_terms(&target.summary) {
        push_anchor_candidate(&mut anchors, &token);
    }
    if let Some(compiled_input) = compiled_input {
        for evidence in compiled_input.selected_evidence.iter().take(8) {
            for token in extract_story_anchor_terms(evidence) {
                push_anchor_candidate(&mut anchors, &token);
            }
        }
    }
    anchors.truncate(16);
    anchors
}

fn extract_story_anchor_terms(text: &str) -> Vec<String> {
    let mut anchors = Vec::new();
    for phrase in [
        "代价", "旧债", "真相", "秘密", "承诺", "背叛", "选择", "入口", "线索",
    ] {
        if text.contains(phrase) {
            push_anchor_candidate(&mut anchors, phrase);
        }
    }

    let chars = text.chars().collect::<Vec<_>>();
    for (idx, ch) in chars.iter().enumerate() {
        if !is_anchor_suffix(*ch) {
            continue;
        }
        let start = idx.saturating_sub(4);
        let mut candidates = Vec::new();
        for prefix_start in start..=idx {
            let candidate = chars[prefix_start..=idx].iter().collect::<String>();
            if !candidate.chars().all(is_anchor_char) {
                continue;
            }
            if let Some(normalized) = normalize_anchor_candidate(&candidate) {
                candidates.push(normalized);
            }
        }
        if let Some(best) = candidates.into_iter().max_by_key(|candidate| char_count(candidate)) {
            push_anchor_candidate(&mut anchors, &best);
        }
    }
    anchors
}

fn normalize_anchor_candidate(candidate: &str) -> Option<String> {
    let mut normalized = candidate.trim().to_string();
    for prefix in [
        "使用", "发现", "拔出", "握住", "回到", "带着", "报告", "私藏", "遭遇", "被迫",
        "继续", "一把", "那个", "这个", "他的", "她的", "它的", "我的", "你的",
    ] {
        if normalized.starts_with(prefix) {
            normalized = normalized.chars().skip(prefix.chars().count()).collect();
        }
    }
    normalized = normalized
        .trim_start_matches(|ch| {
            matches!(
                ch,
                '用'
                    | '向'
                    | '把'
                    | '被'
                    | '将'
                    | '在'
                    | '到'
                    | '从'
                    | '和'
                    | '与'
                    | '的'
                    | '了'
                    | '着'
                    | '过'
                    | '出'
                    | '进'
                    | '回'
                    | '藏'
                    | '拔'
                    | '握'
                    | '看'
                    | '见'
                    | '有'
                    | '是'
                    | '让'
                    | '使'
                    | '令'
                    | '遭'
                    | '遇'
                    | '现'
                    | '告'
                    | '角'
                    | '主'
            )
        })
        .to_string();
    let len = char_count(&normalized);
    if !(2..=8).contains(&len) {
        return None;
    }
    if normalized.chars().any(|ch| {
        matches!(
            ch,
            '在' | '把' | '被' | '将' | '与' | '和' | '到' | '从' | '了' | '着'
        )
    }) {
        return None;
    }
    Some(normalized)
}

fn is_anchor_suffix(ch: char) -> bool {
    matches!(
        ch,
        '剑'
            | '刀'
            | '枪'
            | '弓'
            | '宗'
            | '门'
            | '堂'
            | '城'
            | '山'
            | '谷'
            | '峰'
            | '庙'
            | '墟'
            | '镜'
            | '书'
            | '信'
            | '账'
            | '册'
            | '令'
            | '符'
            | '印'
            | '鼎'
            | '珠'
            | '环'
            | '佩'
            | '玉'
            | '阵'
            | '器'
            | '术'
            | '诀'
            | '丹'
            | '药'
            | '契'
    )
}

fn is_anchor_char(ch: char) -> bool {
    is_cjk(ch) || ch.is_ascii_alphanumeric()
}

fn push_anchor_candidate(anchors: &mut Vec<String>, candidate: &str) {
    let normalized = candidate
        .trim()
        .trim_matches(|ch: char| {
            ch.is_ascii_punctuation()
                || matches!(ch, '：' | '，' | '。' | '、' | '；' | '！' | '？' | '[' | ']')
        })
        .to_string();
    let len = char_count(&normalized);
    if !(2..=12).contains(&len) {
        return;
    }
    if normalized.ends_with('章') && normalized.chars().all(|ch| {
        ch.is_ascii_digit() || matches!(ch, '第' | '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '章')
    }) {
        return;
    }
    if ["chapter", "outline", "lorebook", "json", "previous", "score"].contains(&normalized.as_str())
    {
        return;
    }
    if !anchors.iter().any(|existing| existing == &normalized) {
        anchors.push(normalized);
    }
}

fn build_quality_author_voice_snapshot(
    memory: &crate::writer_agent::memory::WriterMemory,
    project_id: &str,
) -> Option<crate::writer_agent::author_voice::AuthorVoiceSnapshot> {
    let sample_titles = memory
        .list_recent_chapter_results(project_id, 3)
        .unwrap_or_default()
        .into_iter()
        .map(|result| result.chapter_title)
        .filter(|title| !title.trim().is_empty())
        .collect::<Vec<_>>();
    let voice = crate::writer_agent::author_voice::build_author_voice_snapshot(
        memory,
        &sample_titles,
        crate::agent_runtime::now_ms(),
    );
    if voice.confidence <= 0.0 && voice.sample_refs.is_empty() {
        None
    } else {
        Some(voice)
    }
}

fn build_required_story_anchors(
    _project: &dyn ChapterGenerationProject,
    memory: Option<&crate::writer_agent::memory::WriterMemory>,
    target: &ChapterTarget,
    selected_lore: &[(f32, &storage::LoreEntry)],
) -> Vec<StoryAnchor> {
    let mut anchors = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Outline beat keywords
    for term in extract_story_anchor_terms(&target.summary) {
        if seen.insert(term.clone()) {
            anchors.push(StoryAnchor {
                anchor_id: term.clone(),
                source: "outline_beat".to_string(),
                description: format!("大纲节奏关键词: {}", term),
                required: true,
            });
        }
    }

    // 2. Lore entries (not strictly required, but tracked)
    for (_, entry) in selected_lore.iter().take(6) {
        if seen.insert(entry.keyword.clone()) {
            anchors.push(StoryAnchor {
                anchor_id: entry.keyword.clone(),
                source: "lore".to_string(),
                description: snippet_text(&entry.content, 80),
                required: false,
            });
        }
    }

    // 3. Open promises from Story OS
    if let Some(memory) = memory {
        if let Ok(promises) = memory.get_open_promise_summaries() {
            for p in promises.iter().take(8) {
                if seen.insert(p.title.clone()) {
                    anchors.push(StoryAnchor {
                        anchor_id: p.title.clone(),
                        source: "open_promise".to_string(),
                        description: format!("{}: {}", p.kind, p.description),
                        required: true,
                    });
                }
                // Also include related entities from promises
                for entity in &p.related_entities {
                    if seen.insert(entity.clone()) {
                        anchors.push(StoryAnchor {
                            anchor_id: entity.clone(),
                            source: "open_promise".to_string(),
                            description: format!("承诺关联实体: {}", entity),
                            required: true,
                        });
                    }
                }
            }
        }

        // 4. Canon entities from Story OS
        if let Ok(entities) = memory.list_canon_entities() {
            for entity in entities.iter().take(10) {
                if seen.insert(entity.name.clone()) {
                    anchors.push(StoryAnchor {
                        anchor_id: entity.name.clone(),
                        source: "canon_constraint".to_string(),
                        description: format!("{} ({})", entity.summary, entity.kind),
                        required: entity.confidence >= 0.7,
                    });
                }
            }
        }
    }

    anchors.truncate(24);
    anchors
}

fn build_required_state_deltas(
    target: &ChapterTarget,
    intent_artifact: &ChapterIntentArtifact,
    memory: Option<&crate::writer_agent::memory::WriterMemory>,
    next_chapter_summary: Option<&str>,
) -> Vec<StateDelta> {
    let mut deltas = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Helper: scan text for state-change action patterns
    let scan_for_deltas = |text: &str, source: &str, deltas: &mut Vec<StateDelta>, seen: &mut std::collections::HashSet<String>| {
        let mut push = |delta_type: &str, description: &str, source: &str| {
            let key = format!("{}:{}", delta_type, description);
            if seen.insert(key) {
                deltas.push(StateDelta {
                    delta_type: delta_type.to_string(),
                    description: description.to_string(),
                    source: source.to_string(),
                });
            }
        };

        // Knowledge/information deltas
        for (verb, delta_type) in [
            ("发现", "knowledge"), ("揭示", "knowledge"), ("得知", "knowledge"),
            ("了解", "knowledge"), ("明白", "knowledge"), ("知晓", "knowledge"),
            ("泄露", "knowledge"), ("看穿", "knowledge"), ("识破", "knowledge"),
        ] {
            if text.contains(verb) {
                push(delta_type, &format!("角色{}关键信息", verb), source);
            }
        }

        // Relationship deltas
        for (verb, delta_type) in [
            ("背叛", "relationship"), ("信任", "relationship"), ("结盟", "relationship"),
            ("反目", "relationship"), ("和解", "relationship"), ("决裂", "relationship"),
            ("收服", "relationship"), ("投靠", "relationship"),
        ] {
            if text.contains(verb) {
                push(delta_type, &format!("人物关系发生{}", verb), source);
            }
        }

        // Possession deltas
        for (verb, delta_type) in [
            ("获得", "possession"), ("失去", "possession"), ("找到", "possession"),
            ("丢失", "possession"), ("夺取", "possession"), ("交付", "possession"),
            ("继承", "possession"), ("炼化", "possession"), ("觉醒", "possession"),
        ] {
            if text.contains(verb) {
                push(delta_type, &format!("物品/能力{}", verb), source);
            }
        }

        // Status/condition deltas
        for (verb, delta_type) in [
            ("死亡", "status"), ("受伤", "status"), ("康复", "status"),
            ("突破", "status"), ("晋升", "status"), ("堕落", "status"),
            ("蜕变", "status"), ("觉醒", "status"), ("进阶", "status"),
            ("昏迷", "status"), ("复活", "status"),
        ] {
            if text.contains(verb) {
                push(delta_type, &format!("角色状态{}", verb), source);
            }
        }

        // Location deltas
        for (verb, delta_type) in [
            ("逃离", "location"), ("到达", "location"), ("离开", "location"),
            ("进入", "location"), ("潜入", "location"), ("返回", "location"),
            ("传送", "location"), ("降临", "location"),
        ] {
            if text.contains(verb) {
                push(delta_type, &format!("场景位置{}", verb), source);
            }
        }

        // Decision deltas
        for (verb, delta_type) in [
            ("决定", "decision"), ("选择", "decision"), ("答应", "decision"),
            ("拒绝", "decision"), ("承诺", "decision"), ("立誓", "decision"),
        ] {
            if text.contains(verb) {
                push(delta_type, &format!("角色做出{}", verb), source);
            }
        }

        // Conflict deltas
        for (verb, delta_type) in [
            ("击败", "conflict"), ("战胜", "conflict"), ("投降", "conflict"),
            ("逃亡", "conflict"), ("复仇", "conflict"), ("镇压", "conflict"),
            ("反击", "conflict"), ("歼灭", "conflict"),
        ] {
            if text.contains(verb) {
                push(delta_type, &format!("冲突{}", verb), source);
            }
        }
    };

    // 1. Target beat summary
    scan_for_deltas(&target.summary, "outline_beat", &mut deltas, &mut seen);

    // 2. Intent artifact goal
    if !intent_artifact.goal.is_empty() {
        scan_for_deltas(&intent_artifact.goal, "intent_goal", &mut deltas, &mut seen);
    }

    // 3. Open promises — advancing a promise is a state delta
    if let Some(memory) = memory {
        if let Ok(promises) = memory.get_open_promise_summaries() {
            for p in promises.iter().take(6) {
                let key = format!("promise:推进或兑现线索: {}", p.title);
                if seen.insert(key) {
                    deltas.push(StateDelta {
                        delta_type: "promise".to_string(),
                        description: format!("推进或兑现线索: {}", p.title),
                        source: "open_promise".to_string(),
                    });
                }
            }
        }

        // 4. Canon entities — changes to canon are state deltas
        if let Ok(entities) = memory.list_canon_entities() {
            for entity in entities.iter().take(6) {
                if entity.kind.contains("character") || entity.kind.contains("人物") {
                    let key = format!("canon:保持{}设定一致性", entity.name);
                    if seen.insert(key) {
                        deltas.push(StateDelta {
                            delta_type: "canon".to_string(),
                            description: format!("保持{}设定一致性", entity.name),
                            source: "canon_constraint".to_string(),
                        });
                    }
                }
            }
        }
    }

    // 5. Next chapter summary — sets up required state transition
    if let Some(next) = next_chapter_summary {
        scan_for_deltas(next, "next_chapter", &mut deltas, &mut seen);
    }

    deltas.truncate(16);
    deltas
}

fn build_chapter_intent_artifact(
    instruction: &str,
    target: &ChapterTarget,
) -> ChapterIntentArtifact {
    let goal = if instruction.trim().is_empty() {
        format!("Draft '{}'", target.title)
    } else {
        snippet_text(instruction, 220)
    };
    ChapterIntentArtifact {
        chapter_number: target.number,
        chapter_title: Some(target.title.clone()),
        goal,
        must_keep: vec![
            "Respect current outline beat".to_string(),
            "Preserve active canon and promises".to_string(),
        ],
        must_avoid: vec![
            "Do not overwrite dirty editor state".to_string(),
            "Do not skip chapter contract validation".to_string(),
        ],
        style_emphasis: vec![
            "Keep chapter prose only".to_string(),
            "End with a concrete next-beat hook".to_string(),
        ],
    }
}

fn build_selected_evidence_artifact(
    sources: &[ChapterContextSource],
) -> Vec<ChapterSelectedEvidenceArtifact> {
    sources
        .iter()
        .filter(|source| source.included_chars > 0)
        .map(|source| ChapterSelectedEvidenceArtifact {
            source: format!("{}:{}", source.source_type, source.id),
            reason: chapter_source_purpose(&source.source_type).to_string(),
            excerpt: format!(
                "{} contributed {} chars{}",
                source.label,
                source.included_chars,
                if source.truncated { " (truncated)" } else { "" }
            ),
        })
        .collect()
}

fn build_chapter_rule_stack(contract: &ChapterContract) -> ChapterRuleStackArtifact {
    ChapterRuleStackArtifact {
        hard: vec![
            format!(
                "Model output must stay within {}-{} chars",
                contract.min_chars, contract.max_chars
            ),
            format!(
                "Save must stay within {}-{} chars",
                contract.save_hard_floor_chars, contract.save_hard_ceiling_chars
            ),
            "Saving must pass revision/conflict checks".to_string(),
        ],
        soft: vec![
            format!("Aim for {} chars", contract.target_chars),
            "Preserve continuity from recent chapter outcomes".to_string(),
        ],
        diagnostic: vec![
            "Record context budget trace".to_string(),
            "Emit chapter generation run events".to_string(),
        ],
    }
}

pub fn build_chapter_generation_task_packet(
    project_id: &str,
    session_id: &str,
    context: &BuiltChapterContext,
    user_instruction: &str,
    created_at_ms: u64,
) -> TaskPacket {
    let instruction = user_instruction.trim();
    let instruction_summary = if instruction.is_empty() {
        "Draft the target chapter from the built chapter context.".to_string()
    } else {
        snippet_text(instruction, 180)
    };
    let target_title = snippet_text(&context.target.title, 180);
    let objective = snippet_text(
        &format!(
            "Draft '{}' from the chapter generation context. Instruction: {}",
            target_title, instruction_summary
        ),
        560,
    );
    let mut packet = TaskPacket::new(
        format!("{}:{}:ChapterGeneration", session_id, context.request_id),
        objective,
        TaskScope::Chapter,
        created_at_ms,
    );
    packet.scope_ref = Some(context.target.title.clone());
    packet.intent = Some(Intent::GenerateContent);
    packet.constraints = vec![
        "Preserve established canon unless the author explicitly approves a change.".to_string(),
        "Respect the book contract, chapter mission, outline beat, and known promise ledger."
            .to_string(),
        "Generate chapter prose only; no analysis, markdown fences, or meta commentary."
            .to_string(),
        format!(
            "Target chapter length is {} chars, acceptable model-output range is {}-{} chars, and save floor/ceiling is {}-{} chars.",
            context.chapter_contract.target_chars,
            context.chapter_contract.min_chars,
            context.chapter_contract.max_chars,
            context.chapter_contract.save_hard_floor_chars,
            context.chapter_contract.save_hard_ceiling_chars
        ),
        "Saving generated content must pass revision/conflict checks before overwriting chapters."
            .to_string(),
    ];
    packet.success_criteria = vec![
        "Generated prose passes non-empty, size, and chapter contract validation.".to_string(),
        "Context sources include the instruction plus chapter/continuity memory before drafting."
            .to_string(),
        "Save completes, or a concrete save conflict is surfaced to the author.".to_string(),
        "Chapter result feedback can be recorded after a successful save.".to_string(),
    ];
    packet.beliefs = chapter_context_beliefs(context, project_id);
    packet.required_context = chapter_required_context(context);
    packet.tool_policy = ToolPolicyContract {
        max_side_effect_level: ToolSideEffectLevel::Write,
        allow_approval_required: true,
        required_tool_tags: vec!["generation".to_string()],
    };
    packet.feedback = FeedbackContract {
        expected_signals: vec![
            PHASE_CONTEXT_BUILT.to_string(),
            PHASE_COMPLETED.to_string(),
            PHASE_CONFLICT.to_string(),
            "chapter_result_summary".to_string(),
        ],
        checkpoints: vec![
            "record chapter generation context sources".to_string(),
            "validate generated content before save".to_string(),
            "check target revision before overwrite".to_string(),
            "record result feedback after successful save".to_string(),
        ],
        memory_writes: vec![
            "chapter_result_summary".to_string(),
            "outline_status".to_string(),
        ],
    };
    packet
}

fn build_target_beat_context(summary: &str) -> String {
    let primary = infer_primary_objective(summary);
    let hold_back = infer_hold_back_reveal(summary);
    let pressure = infer_scene_pressure(summary);
    let payoff = infer_required_payoff(summary);

    let mut lines = vec![format!("Beat summary: {}", compact_line(summary, 180))];
    lines.push(format!("Primary objective: {}", primary));
    if let Some(pressure) = pressure {
        lines.push(format!("Immediate pressure: {}", pressure));
    }
    if let Some(payoff) = payoff {
        lines.push(format!("Required payoff or partial payoff: {}", payoff));
    }
    if let Some(hold_back) = hold_back {
        lines.push(format!("Hold-back reveal: {}", hold_back));
    }
    lines.join("\n")
}

fn infer_primary_objective(summary: &str) -> String {
    if contains_any(summary, &["进入", "潜入", "抵达"]) {
        "complete the immediate entry/action step before widening into lore exposition".to_string()
    } else if contains_any(summary, &["对峙", "逼问", "抢"]) {
        "force a concrete confrontation and decision in-scene".to_string()
    } else if contains_any(summary, &["交易", "交换", "选择"]) {
        "make the scene hinge on a costly choice, not explanation only".to_string()
    } else {
        "advance one concrete scene objective before expanding world explanation".to_string()
    }
}

fn infer_hold_back_reveal(summary: &str) -> Option<String> {
    if contains_any(summary, &["真相", "身份", "封门", "原因", "意识到"]) {
        Some(
            "move the truth closer through evidence or image, but do not fully explain the full sealing truth in the same chapter"
                .to_string(),
        )
    } else {
        None
    }
}

fn infer_scene_pressure(summary: &str) -> Option<String> {
    if contains_any(summary, &["倒影", "镜中墟", "入口"]) {
        Some("keep the scene anchored in the unstable threshold / mirror encounter, not wide retrospective exposition".to_string())
    } else if contains_any(summary, &["宗门", "抢", "追兵"]) {
        Some("external arrival should force action quickly".to_string())
    } else {
        None
    }
}

fn infer_required_payoff(summary: &str) -> Option<String> {
    if contains_any(summary, &["旧债", "背叛", "交易", "承认"]) {
        Some("pay at least one slice of emotional debt or trust pressure inside the scene".to_string())
    } else {
        None
    }
}

pub fn build_chapter_generation_receipt(
    request_id: &str,
    target: &ChapterTarget,
    base_revision: &str,
    user_instruction: &str,
    sources: &[ChapterContextSource],
    created_at_ms: u64,
) -> WriterTaskReceipt {
    let instruction = user_instruction.trim();
    let objective = if instruction.is_empty() {
        format!("Draft '{}' from the built chapter context.", target.title)
    } else {
        format!(
            "Draft '{}' from the built chapter context. Instruction: {}",
            target.title,
            snippet_text(instruction, 180)
        )
    };
    let mut required_evidence = vec!["instruction".to_string()];
    for source in sources.iter().filter(|source| source.included_chars > 0) {
        if is_required_chapter_source(&source.source_type)
            && !required_evidence
                .iter()
                .any(|existing| existing == &source.source_type)
        {
            required_evidence.push(source.source_type.clone());
        }
    }
    let source_refs = sources
        .iter()
        .filter(|source| source.included_chars > 0)
        .map(|source| format!("{}:{}", source.source_type, source.id))
        .collect::<Vec<_>>();

    WriterTaskReceipt::new(
        request_id,
        "ChapterGeneration",
        Some(target.title.clone()),
        objective,
        required_evidence,
        vec!["chapter_draft".to_string(), "saved_chapter".to_string()],
        vec![
            "overwrite_without_revision_match".to_string(),
            "change_target_chapter_without_new_receipt".to_string(),
            "ignore_required_context_sources".to_string(),
        ],
        source_refs,
        Some(base_revision.to_string()),
        created_at_ms,
    )
}

fn chapter_context_beliefs(context: &BuiltChapterContext, project_id: &str) -> Vec<TaskBelief> {
    let mut beliefs = context
        .sources
        .iter()
        .filter(|source| source.included_chars > 0)
        .take(8)
        .map(|source| {
            let mut statement = format!(
                "{} contributes {} chars",
                source.label, source.included_chars
            );
            if source.truncated {
                statement.push_str(" after truncation");
            }
            TaskBelief::new(
                source.source_type.clone(),
                statement,
                chapter_source_confidence(&source.source_type),
            )
            .with_source(source.id.clone())
        })
        .collect::<Vec<_>>();

    if beliefs.is_empty() {
        beliefs.push(
            TaskBelief::new(
                "chapter_generation_context",
                format!(
                    "{} has no explicit context sources; fall back to project {}.",
                    context.target.title, project_id
                ),
                0.5,
            )
            .with_source(context.request_id.clone()),
        );
    }

    beliefs
}

fn chapter_required_context(context: &BuiltChapterContext) -> Vec<RequiredContext> {
    let mut required_context = context
        .sources
        .iter()
        .take(12)
        .map(|source| {
            RequiredContext::new(
                source.source_type.clone(),
                chapter_source_purpose(&source.source_type),
                source.included_chars.max(1),
                is_required_chapter_source(&source.source_type),
            )
        })
        .collect::<Vec<_>>();

    if !required_context
        .iter()
        .any(|context| context.required && !context.source_type.trim().is_empty())
    {
        required_context.push(RequiredContext::new(
            "chapter_generation_context",
            "Fallback chapter context required to draft safely.",
            1,
            true,
        ));
    }

    required_context
}

fn is_required_chapter_source(source_type: &str) -> bool {
    matches!(
        source_type,
        "instruction"
            | "outline"
            | "target_beat"
            | "previous_chapters"
            | "lorebook"
            | "project_brain"
    )
}

fn chapter_source_purpose(source_type: &str) -> &'static str {
    match source_type {
        "instruction" => "Capture the author's explicit generation request.",
        "outline" => "Keep the draft aligned with the book-level beat sheet.",
        "target_beat" => "Preserve the target chapter mission and planned payoff.",
        "previous_chapters" => "Maintain continuity from recent chapter outcomes.",
        "next_chapter" => "Avoid blocking the next planned beat.",
        "target_existing_text" => "Respect any existing prose already in the target chapter.",
        "lorebook" => "Ground character, setting, and canon details.",
        "project_brain" => "Recall relevant long-range project memory.",
        "user_profile" => "Preserve learned author style preferences.",
        _ => "Provide supporting context for chapter generation.",
    }
}

fn chapter_source_confidence(source_type: &str) -> f32 {
    match source_type {
        "instruction" | "target_beat" => 0.92,
        "outline" | "lorebook" => 0.88,
        "previous_chapters" | "project_brain" => 0.78,
        "next_chapter" | "target_existing_text" | "user_profile" => 0.70,
        _ => 0.60,
    }
}

fn snippet_text(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Tracks what changed between calls so FocusPack is only rebuilt when needed.
#[derive(Default)]
pub struct FocusState {
    last_chapter: String,
    last_scene_id: Option<i64>,
    last_result_hash: String,
    last_next_beat_hash: String,
    pub rebuild_count: usize,
}

impl FocusState {
    pub fn needs_rebuild(
        &self,
        chapter: &str,
        scene_id: Option<i64>,
        result_hash: &str,
        next_beat_hash: &str,
    ) -> bool {
        self.last_chapter != chapter
            || self.last_scene_id != scene_id
            || self.last_result_hash != result_hash
            || self.last_next_beat_hash != next_beat_hash
    }

    pub fn record_rebuild(
        &mut self,
        chapter: &str,
        scene_id: Option<i64>,
        result_hash: &str,
        next_beat_hash: &str,
    ) {
        self.last_chapter = chapter.to_string();
        self.last_scene_id = scene_id;
        self.last_result_hash = result_hash.to_string();
        self.last_next_beat_hash = next_beat_hash.to_string();
        self.rebuild_count = self.rebuild_count.wrapping_add(1);
    }
}

static FOCUS_STATE: std::sync::OnceLock<std::sync::Mutex<FocusState>> = std::sync::OnceLock::new();

fn focus_state() -> &'static std::sync::Mutex<FocusState> {
    FOCUS_STATE.get_or_init(|| std::sync::Mutex::new(FocusState::default()))
}

/// Cache-aware context spine for chapter generation.
/// Layers ordered from most cache-stable to most volatile.
#[derive(Debug, Clone, Default)]
pub struct ChapterContextSpine {
    pub frozen_prefix: String,
    pub project_stable: String,
    pub focus_pack: String,
    pub hot_buffer: String,
    pub ephemeral: String,
}

impl ChapterContextSpine {
    pub fn prefix_char_count(&self) -> usize {
        self.frozen_prefix.chars().count() + self.project_stable.chars().count()
    }

    pub fn tail_char_count(&self) -> usize {
        self.focus_pack.chars().count()
            + self.hot_buffer.chars().count()
            + self.ephemeral.chars().count()
    }

    pub fn total_chars(&self) -> usize {
        self.frozen_prefix.chars().count()
            + self.project_stable.chars().count()
            + self.focus_pack.chars().count()
            + self.hot_buffer.chars().count()
            + self.ephemeral.chars().count()
    }
}

pub fn build_chapter_generation_spine(
    target: &ChapterTarget,
    contract: Option<&crate::writer_agent::memory::StoryContractSummary>,
    mission: Option<&crate::writer_agent::memory::ChapterMissionSummary>,
    result_feedback: Option<&crate::writer_agent::memory::ChapterResultSummary>,
    compiled_input: Option<&CompiledInput>,
    _memory: &crate::writer_agent::memory::WriterMemory,
) -> ChapterContextSpine {
    let mut spine = ChapterContextSpine {
        frozen_prefix: format!(
            "Chapter generation contract for '{}'. Output: chapter text only.",
            target.title
        ),
        ..Default::default()
    };

    if let Some(c) = contract {
        spine.project_stable = format!(
            "Story: {} — {} | {}",
            c.genre, c.main_conflict, c.tone_contract
        );
    }

    let mut focus = String::new();
    if let Some(m) = mission {
        focus.push_str(&format!("Mission: {}\n", m.mission));
    }
    if let Some(rf) = result_feedback {
        focus.push_str(&format!("Previous result: {}\n", rf.summary));
    }
    if let Some(ci) = compiled_input {
        focus.push_str(&format!(
            "Plan: {}\nEvidence: {}\nRules: {}",
            ci.intent_text,
            ci.selected_evidence.join("; "),
            ci.rule_stack.join("; ")
        ));
    }
    spine.focus_pack = focus;

    spine.hot_buffer = format!("Target: {}", target.title);

    spine
}
