
use crate::writer_agent::memory::{
    ArcSnapshotSummary, BookStateSummary, VolumeSnapshotSummary, VolumeSummary,
};

/// Assembles a ContextPack under strict budget constraints.
pub fn assemble_context_pack(
    task: AgentTask,
    source_provider: &dyn Fn(ContextSource) -> Option<String>,
    total_budget: usize,
) -> WritingContextPack {
    let priorities = task.source_priorities();
    let mut sources = Vec::new();
    let mut used = 0usize;
    let mut source_reports = Vec::new();
    let mut raw_sources = Vec::new();

    for (source, priority, budget) in priorities {
        if let Some(raw) = source_provider(source.clone()) {
            raw_sources.push(SourceDraft {
                source,
                priority,
                requested: budget,
                raw,
                required_budget: 0,
                consumed: 0,
            });
        }
    }

    for (required_source, required_budget) in task.required_source_budgets() {
        if let Some(draft) = raw_sources
            .iter_mut()
            .find(|draft| draft.source == required_source)
        {
            draft.required_budget = required_budget.min(draft.requested);
        }
    }

    for draft in raw_sources
        .iter_mut()
        .filter(|draft| draft.required_budget > 0)
    {
        let remaining = total_budget.saturating_sub(used);
        let alloc = draft.required_budget.min(remaining);
        if alloc == 0 {
            break;
        }

        let content = char_window(&draft.raw, draft.consumed, alloc);
        let char_count = content.chars().count();
        if char_count == 0 {
            continue;
        }
        draft.consumed += char_count;
        used += char_count;
        sources.push(ContextExcerpt {
            source: draft.source.clone(),
            content,
            char_count,
            truncated: draft.raw.chars().count() > draft.consumed,
            priority: draft.priority,
            evidence_ref: None,
        });
    }

    for draft in raw_sources.iter_mut() {
        let remaining = total_budget.saturating_sub(used);
        let requested_remaining = draft.requested.saturating_sub(draft.consumed);
        let alloc = requested_remaining.min(remaining);
        if alloc == 0 {
            continue;
        }

        let content = char_window(&draft.raw, draft.consumed, alloc);
        let char_count = content.chars().count();
        if char_count == 0 {
            continue;
        }
        draft.consumed += char_count;
        used += char_count;

        if let Some(existing) = sources
            .iter_mut()
            .find(|source| source.source == draft.source)
        {
            existing.content.push_str(&content);
            existing.char_count += char_count;
            existing.truncated = draft.raw.chars().count() > draft.consumed;
        } else {
            sources.push(ContextExcerpt {
                source: draft.source.clone(),
                content,
                char_count,
                truncated: draft.raw.chars().count() > draft.consumed,
                priority: draft.priority,
                evidence_ref: None,
            });
        }
    }

    for draft in raw_sources {
        if draft.consumed > 0 || draft.raw.chars().count() > 0 {
            source_reports.push(SourceReport {
                source: format!("{:?}", draft.source),
                requested: draft.requested,
                provided: draft.consumed,
                truncated: draft.raw.chars().count() > draft.consumed,
                reason: source_inclusion_reason(&task, &draft),
                truncation_reason: source_truncation_reason(total_budget, &draft),
            });
        }
    }
    sources.sort_by_key(|right| std::cmp::Reverse(right.priority));

    WritingContextPack {
        task,
        total_chars: used,
        budget_limit: total_budget,
        budget_report: ContextBudgetReport {
            total_budget,
            used,
            wasted: total_budget.saturating_sub(used),
            source_reports,
        },
        sources,
    }
}

pub fn append_context_source_with_budget(
    pack: &mut WritingContextPack,
    source: ContextSource,
    content: String,
    requested: usize,
    priority: u8,
    evidence_ref: Option<String>,
) {
    if content.trim().is_empty() {
        return;
    }

    let remaining = pack.budget_limit.saturating_sub(pack.total_chars);
    let alloc = requested.min(remaining);
    let included = char_window(&content, 0, alloc);
    let provided = included.chars().count();
    if provided == 0 {
        pack.budget_report.source_reports.push(SourceReport {
            source: format!("{:?}", source),
            requested,
            provided: 0,
            truncated: content.chars().count() > 0,
            reason: format!(
                "{:?} derived source was dropped because the context budget was exhausted before allocation.",
                pack.task
            ),
            truncation_reason: Some(format!(
                "ContextPack total budget of {} chars was exhausted before this source could be included.",
                pack.budget_limit
            )),
        });
        return;
    }

    pack.total_chars += provided;
    pack.budget_report.used += provided;
    pack.budget_report.wasted = pack.budget_limit.saturating_sub(pack.total_chars);
    let truncated = content.chars().count() > provided;
    pack.sources.push(ContextExcerpt {
        source: source.clone(),
        content: included,
        char_count: provided,
        truncated,
        priority,
        evidence_ref,
    });
    pack.sources
        .sort_by_key(|right| std::cmp::Reverse(right.priority));
    pack.budget_report.source_reports.push(SourceReport {
        source: format!("{:?}", source),
        requested,
        provided,
        truncated,
        reason: format!(
            "{:?} derived source included after story impact radius calculation.",
            pack.task
        ),
        truncation_reason: if truncated {
            Some(format!(
                "Source content was limited by remaining ContextPack budget of {} chars.",
                provided
            ))
        } else {
            None
        },
    });
}

struct SourceDraft {
    source: ContextSource,
    priority: u8,
    requested: usize,
    raw: String,
    required_budget: usize,
    consumed: usize,
}

fn source_inclusion_reason(task: &AgentTask, draft: &SourceDraft) -> String {
    if draft.consumed == 0 {
        return if draft.required_budget > 0 {
            format!(
                "{:?} required source could not be included because the context budget was exhausted.",
                task
            )
        } else {
            format!(
                "{:?} priority {} source was dropped because the context budget was exhausted before allocation.",
                task, draft.priority
            )
        };
    }

    if draft.required_budget > 0 {
        format!(
            "{:?} required source reserved {} chars before priority fill.",
            task, draft.required_budget
        )
    } else {
        format!(
            "{:?} priority {} source included during remaining-budget fill.",
            task, draft.priority
        )
    }
}

fn source_truncation_reason(total_budget: usize, draft: &SourceDraft) -> Option<String> {
    let raw_chars = draft.raw.chars().count();
    if raw_chars <= draft.consumed {
        return None;
    }

    if draft.consumed >= draft.requested {
        Some(format!(
            "Source content was limited by its per-source budget of {} chars.",
            draft.requested
        ))
    } else {
        Some(format!(
            "ContextPack total budget of {} chars was exhausted before this source could be fully included.",
            total_budget
        ))
    }
}

pub fn query_story_os(
    task: AgentTask,
    observation: &WriterObservation,
    memory: &WriterMemory,
    total_budget: usize,
) -> WritingContextPack {
    let chapter_number = observation
        .chapter_title
        .as_deref()
        .and_then(chapter_number_from_title);
    let active_volume = chapter_number
        .and_then(|number| memory.find_volume_for_chapter(&observation.project_id, number).ok().flatten());
    let book_state = memory.get_book_state(&observation.project_id).ok().flatten();
    let volume_snapshots = related_volume_snapshots(
        &observation.project_id,
        active_volume.as_ref(),
        memory,
    );
    let arc_snapshots = related_arc_snapshots(&observation.project_id, active_volume.as_ref(), memory);

    let project_brief = build_project_brief(&observation.project_id, memory);
    let chapter_mission = build_chapter_mission(&observation.project_id, observation, memory);
    let next_beat = build_next_beat(&observation.project_id, observation, memory);
    let result_feedback = build_result_feedback(&observation.project_id, observation, memory);
    let decisions = memory.list_recent_decisions(6).unwrap_or_default();
    let all_open_promises = memory.get_open_promise_summaries().unwrap_or_default();
    let open_promises =
        prefilter_promises_for_story_os(observation, active_volume.as_ref(), &all_open_promises);
    let decision_slice = build_decision_slice(&decisions);
    let relevance = WritingRelevance::new(
        observation,
        &chapter_mission,
        &next_beat,
        &result_feedback,
        &decision_slice,
    );
    let canon_slice = build_canon_slice(observation, memory, &relevance, &open_promises);
    let promise_slice = build_promise_slice(observation, &open_promises, &relevance, &decisions, memory);
    let author_style = build_style_slice(memory);
    let book_state_text = build_book_state_context(book_state.as_ref());
    let arc_snapshot_text = build_arc_snapshot_context(&arc_snapshots);
    let volume_snapshot_text = build_volume_snapshot_context(&volume_snapshots);
    let selected_text = observation.selected_text().to_string();
    let cursor_prefix = if observation.prefix.trim().is_empty() {
        observation.paragraph.clone()
    } else {
        observation.prefix.clone()
    };
    let cursor_suffix = observation.suffix.clone();
    let reader_compensation =
        build_reader_compensation_context(&observation.project_id, observation, memory);

    assemble_context_pack(
        task,
        &|source| match source {
            ContextSource::CursorPrefix => non_empty(cursor_prefix.clone()),
            ContextSource::CursorSuffix => non_empty(cursor_suffix.clone()),
            ContextSource::SelectedText => non_empty(selected_text.clone()),
            ContextSource::ProjectBrief => non_empty(project_brief.clone()),
            ContextSource::BookState => non_empty(book_state_text.clone()),
            ContextSource::ArcSnapshot => non_empty(arc_snapshot_text.clone()),
            ContextSource::VolumeSnapshot => non_empty(volume_snapshot_text.clone()),
            ContextSource::ChapterMission => non_empty(chapter_mission.clone()),
            ContextSource::NextBeat => non_empty(next_beat.clone()),
            ContextSource::ResultFeedback => non_empty(result_feedback.clone()),
            ContextSource::CanonSlice => non_empty(canon_slice.clone()),
            ContextSource::PromiseSlice => non_empty(promise_slice.clone()),
            ContextSource::DecisionSlice => non_empty(decision_slice.clone()),
            ContextSource::AuthorStyle => non_empty(author_style.clone()),
            ContextSource::StoryImpactRadius => None,
            ContextSource::ReaderCompensation => non_empty(reader_compensation.clone()),
            _ => None,
        },
        total_budget,
    )
}

pub fn assemble_observation_context(
    task: AgentTask,
    observation: &WriterObservation,
    memory: &WriterMemory,
    total_budget: usize,
) -> WritingContextPack {
    query_story_os(task, observation, memory, total_budget)
}

fn build_book_state_context(book_state: Option<&BookStateSummary>) -> String {
    let Some(book_state) = book_state else {
        return String::new();
    };
    let mut lines = Vec::new();
    push_contract_line(&mut lines, "全书标题", &book_state.title);
    if !book_state.long_term_constraints.is_empty() {
        lines.push(format!(
            "长期约束: {}",
            book_state.long_term_constraints.join(" / ")
        ));
    }
    if !book_state.mega_promises.is_empty() {
        lines.push(format!("全书长线承诺: {}", book_state.mega_promises.join(" / ")));
    }
    if !book_state.irreversible_changes.is_empty() {
        lines.push(format!(
            "不可逆变化: {}",
            book_state.irreversible_changes.join(" / ")
        ));
    }
    lines.join("\n")
}

fn build_arc_snapshot_context(arcs: &[ArcSnapshotSummary]) -> String {
    arcs.iter()
        .take(2)
        .map(|arc| {
            format!(
                "[{} {}-{}]\n{}",
                arc.title,
                arc.start_chapter,
                arc.end_chapter,
                compact_story_snapshot(&arc.snapshot)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn build_volume_snapshot_context(volumes: &[VolumeSnapshotSummary]) -> String {
    volumes
        .iter()
        .take(3)
        .map(|volume| {
            format!(
                "[{}]\n{}",
                volume.volume_id,
                compact_story_snapshot(&volume.snapshot)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn compact_story_snapshot(snapshot: &serde_json::Value) -> String {
    match snapshot {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Object(map) => map
            .iter()
            .take(6)
            .map(|(key, value)| format!("{}: {}", key, compact_json_value(value)))
            .collect::<Vec<_>>()
            .join(" | "),
        _ => compact_json_value(snapshot),
    }
}

fn compact_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .take(4)
            .map(compact_json_value)
            .collect::<Vec<_>>()
            .join(", "),
        _ => value.to_string(),
    }
}

fn related_volume_snapshots(
    project_id: &str,
    active_volume: Option<&VolumeSummary>,
    memory: &WriterMemory,
) -> Vec<VolumeSnapshotSummary> {
    let Some(active_volume) = active_volume else {
        return Vec::new();
    };
    let volumes = memory.list_volumes(project_id).unwrap_or_default();
    volumes
        .into_iter()
        .filter(|volume| volume.start_chapter <= active_volume.start_chapter)
        .rev()
        .take(3)
        .filter_map(|volume| memory.get_latest_volume_snapshot(project_id, &volume.id).ok().flatten())
        .collect()
}

fn related_arc_snapshots(
    project_id: &str,
    active_volume: Option<&VolumeSummary>,
    memory: &WriterMemory,
) -> Vec<ArcSnapshotSummary> {
    let Some(active_volume) = active_volume else {
        return Vec::new();
    };
    memory
        .list_arc_snapshots(project_id, &active_volume.id)
        .unwrap_or_default()
}

fn prefilter_promises_for_story_os(
    observation: &WriterObservation,
    active_volume: Option<&VolumeSummary>,
    promises: &[PlotPromiseSummary],
) -> Vec<PlotPromiseSummary> {
    let current_chapter = observation.chapter_title.as_deref().unwrap_or_default();
    let current_number = chapter_number_from_title(current_chapter);
    let mut selected = promises
        .iter()
        .filter(|promise| {
            if promise.expected_payoff.contains(current_chapter) {
                return true;
            }
            if let (Some(volume), Some(introduced)) = (
                active_volume,
                chapter_number_from_title(&promise.introduced_chapter),
            ) {
                return introduced >= volume.start_chapter && introduced <= volume.end_chapter;
            }
            if let (Some(volume), Some(last_seen)) = (
                active_volume,
                chapter_number_from_title(&promise.last_seen_chapter),
            ) {
                return last_seen >= volume.start_chapter && last_seen <= volume.end_chapter;
            }
            if let (Some(now), Some(payoff)) = (
                current_number,
                chapter_number_from_title(&promise.expected_payoff),
            ) {
                return (now - payoff).abs() <= 8;
            }
            false
        })
        .cloned()
        .collect::<Vec<_>>();
    if selected.len() < 30 {
        for promise in promises.iter().take(30) {
            if selected.iter().any(|existing| existing.id == promise.id) {
                continue;
            }
            selected.push(promise.clone());
            if selected.len() >= 30 {
                break;
            }
        }
    }
    selected
}

fn chapter_number_from_title(chapter: &str) -> Option<i64> {
    let digits = chapter.chars().filter(|ch| ch.is_ascii_digit()).collect::<String>();
    if !digits.is_empty() {
        return digits.parse::<i64>().ok();
    }

    let start = chapter.find('第')?;
    let rest = &chapter[start + '第'.len_utf8()..];
    let end = rest.find('章').unwrap_or(rest.len());
    let raw = rest[..end].trim();
    parse_chinese_number(raw)
}

fn parse_chinese_number(raw: &str) -> Option<i64> {
    if raw.is_empty() {
        return None;
    }
    let digit = |ch: char| match ch {
        '零' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    };
    if raw == "十" {
        return Some(10);
    }
    if let Some(idx) = raw.find('十') {
        let left = raw[..idx].chars().next().and_then(digit).unwrap_or(1);
        let right = raw[idx + '十'.len_utf8()..]
            .chars()
            .next()
            .and_then(digit)
            .unwrap_or(0);
        return Some((left * 10 + right) as i64);
    }
    let mut value = 0i64;
    for ch in raw.chars() {
        value = value * 10 + i64::from(digit(ch)?);
    }
    Some(value)
}

fn build_chapter_mission(
    project_id: &str,
    observation: &WriterObservation,
    memory: &WriterMemory,
) -> String {
    observation
        .chapter_title
        .as_deref()
        .and_then(|chapter| {
            memory
                .get_chapter_mission(project_id, chapter)
                .ok()
                .flatten()
        })
        .filter(|mission| !mission.is_empty())
        .map(|mission| mission.render_for_context())
        .unwrap_or_default()
}

fn build_next_beat(
    project_id: &str,
    observation: &WriterObservation,
    memory: &WriterMemory,
) -> String {
    let active_mission = observation.chapter_title.as_deref().and_then(|chapter| {
        memory
            .get_chapter_mission(project_id, chapter)
            .ok()
            .flatten()
    });
    let recent_results = memory
        .list_recent_chapter_results(project_id, 6)
        .unwrap_or_default();
    let open_promises = memory.get_open_promise_summaries().unwrap_or_default();
    derive_next_beat(
        observation.chapter_title.as_deref(),
        active_mission.as_ref(),
        &recent_results,
        &open_promises,
    )
    .filter(|beat| !beat.is_empty())
    .map(|beat| beat.render_for_context())
    .unwrap_or_default()
}

fn build_result_feedback(
    project_id: &str,
    observation: &WriterObservation,
    memory: &WriterMemory,
) -> String {
    let mut results = memory
        .list_recent_chapter_results(project_id, 4)
        .unwrap_or_default()
        .into_iter()
        .filter(|result| !result.is_empty())
        .collect::<Vec<_>>();

    if let Some(current_chapter) = observation.chapter_title.as_deref() {
        results.retain(|result| {
            result.chapter_title != current_chapter
                || observation.reason == crate::writer_agent::observation::ObservationReason::Save
        });
    }

    results
        .into_iter()
        .take(3)
        .map(|result| result.render_for_context())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn build_project_brief(project_id: &str, memory: &WriterMemory) -> String {
    memory
        .get_story_contract(project_id)
        .ok()
        .flatten()
        .filter(|contract| contract.quality() >= StoryContractQuality::Usable)
        .map(|contract| contract.render_for_context())
        .unwrap_or_default()
}


fn build_reader_compensation_context(
    project_id: &str,
    observation: &WriterObservation,
    memory: &WriterMemory,
) -> String {
    let mut lines = Vec::new();
    if let Ok(Some(profile)) = memory.get_reader_compensation_profile(project_id) {
        if !profile.target_reader.trim().is_empty() {
            push_contract_line(&mut lines, "目标读者", &profile.target_reader);
        }
        if !profile.primary_lack.trim().is_empty() {
            push_contract_line(&mut lines, "主缺口", &profile.primary_lack);
        }
        if !profile.dominant_relationship_soil.trim().is_empty() {
            push_contract_line(&mut lines, "关系土壤", &profile.dominant_relationship_soil);
        }
    }
    if let Some(chapter) = observation.chapter_title.as_deref() {
        if let Ok(Some(mission)) = memory.get_chapter_mission(project_id, chapter) {
            if !mission.pressure_scene.trim().is_empty() {
                push_contract_line(&mut lines, "本章压迫", &mission.pressure_scene);
            }
            if !mission.payoff_target.trim().is_empty() {
                push_contract_line(&mut lines, "本章补偿目标", &mission.payoff_target);
            }
            if !mission.next_lack_opened.trim().is_empty() {
                push_contract_line(&mut lines, "下一层缺口", &mission.next_lack_opened);
            }
        }
    }
    if let Ok(debts) = memory.get_open_emotional_debts(project_id) {
        let recent: Vec<_> = debts.iter().take(3).collect();
        if !recent.is_empty() {
            let debt_lines: Vec<_> = recent
                .iter()
                .map(|d| format!("  {} ({}) - {}", d.title, d.debt_kind, d.payoff_status))
                .collect();
            lines.push(format!("活跃情绪债务:\n{}", debt_lines.join("\n")));
        }
    }
    lines.join("\n")
}
