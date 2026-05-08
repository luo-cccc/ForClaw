pub fn seed_chapter_missions_from_outline(
    project_id: &str,
    outline: &[crate::storage::OutlineNode],
    memory: &WriterMemory,
) -> Result<usize, String> {
    let mut seeded = 0usize;
    for node in outline
        .iter()
        .filter(|node| !node.chapter_title.trim().is_empty())
    {
        let summary = compact_context_line(&node.summary, 180);
        let mission = if summary.is_empty() {
            format!(
                "推进 {} 的章节目标，并保持与书级合同一致。",
                node.chapter_title
            )
        } else {
            summary.clone()
        };
        let must_include = infer_mission_must_include(&node.summary);
        let must_not = infer_mission_must_not(&node.summary);
        let expected_ending = infer_mission_expected_ending(&node.summary);
        let did_seed = memory
            .ensure_chapter_mission_seed(
                project_id,
                &node.chapter_title,
                &mission,
                &must_include,
                &must_not,
                &expected_ending,
                "outline.seed",
            )
            .map_err(|e| e.to_string())?;
        if did_seed {
            seeded += 1;
        }
    }
    Ok(seeded)
}

fn infer_mission_must_include(summary: &str) -> String {
    let mut items = Vec::new();
    if contains_any(summary, &["伏笔", "线索", "玉佩", "密道", "钥匙"]) {
        items.push("保留并推进关键线索");
    }
    if contains_any(summary, &["冲突", "对抗", "危机", "敌"]) {
        items.push("让冲突产生可见后果");
    }
    if contains_any(summary, &["关系", "信任", "背叛", "误会"]) {
        items.push("推进角色关系状态变化");
    }
    if items.is_empty() {
        "保持本章目标与大纲摘要一致".to_string()
    } else {
        items.join("；")
    }
}

fn infer_mission_must_not(summary: &str) -> String {
    let mut items = Vec::new();
    if contains_any(summary, &["谜", "真相", "秘密", "身份"]) {
        items.push("不要过早揭开核心谜底");
    }
    if contains_any(summary, &["试探", "怀疑", "误会"]) {
        items.push("不要让角色过早达成完全信任");
    }
    if items.is_empty() {
        "不要跳过因果铺垫或改写已确认设定".to_string()
    } else {
        items.join("；")
    }
}

fn infer_mission_expected_ending(summary: &str) -> String {
    if contains_any(summary, &["危机", "追杀", "敌", "对抗"]) {
        "以新的压力、危险或选择收束。".to_string()
    } else if contains_any(summary, &["线索", "发现", "秘密", "谜"]) {
        "以新的线索或疑问收束。".to_string()
    } else if contains_any(summary, &["关系", "信任", "背叛", "误会"]) {
        "以角色关系状态变化收束。".to_string()
    } else {
        "以明确的状态变化或下一步钩子收束。".to_string()
    }
}

pub fn seed_story_contract_from_project_assets(
    project_id: &str,
    project_name: &str,
    lorebook: &[crate::storage::LoreEntry],
    outline: &[crate::storage::OutlineNode],
    memory: &WriterMemory,
) -> Result<bool, String> {
    let title = if project_name.trim().is_empty() {
        "Untitled Story"
    } else {
        project_name.trim()
    };
    let genre = infer_contract_genre(lorebook, outline);
    let reader_promise = infer_reader_promise(outline);
    let main_conflict = infer_main_conflict(outline, lorebook);
    let structural_boundary = infer_structural_boundary(lorebook, outline);
    memory
        .ensure_story_contract_seed(
            project_id,
            title,
            &genre,
            &reader_promise,
            &main_conflict,
            &structural_boundary,
        )
        .map_err(|e| e.to_string())
}

fn infer_contract_genre(
    lorebook: &[crate::storage::LoreEntry],
    outline: &[crate::storage::OutlineNode],
) -> String {
    let haystack = project_asset_haystack(lorebook, outline);
    if contains_any(&haystack, &["玄幻", "修仙", "灵力", "宗门", "秘境"]) {
        "玄幻/修仙".to_string()
    } else if contains_any(&haystack, &["悬疑", "案件", "凶手", "线索", "侦探"]) {
        "悬疑".to_string()
    } else if contains_any(&haystack, &["末日", "丧尸", "废土", "灾变"]) {
        "末日/废土".to_string()
    } else if contains_any(&haystack, &["星舰", "宇宙", "机甲", "AI", "人工智能"]) {
        "科幻".to_string()
    } else if contains_any(&haystack, &["宫廷", "朝堂", "皇帝", "王府", "江湖"]) {
        "古风/权谋".to_string()
    } else {
        "待定长篇小说".to_string()
    }
}

fn infer_reader_promise(outline: &[crate::storage::OutlineNode]) -> String {
    let first_nodes = outline
        .iter()
        .take(3)
        .map(|node| compact_context_line(&node.summary, 80))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if first_nodes.is_empty() {
        "保持主线清晰、角色选择有后果，并让每章都推动故事状态。".to_string()
    } else {
        format!("围绕开篇承诺推进: {}", first_nodes.join(" / "))
    }
}

fn infer_main_conflict(
    outline: &[crate::storage::OutlineNode],
    lorebook: &[crate::storage::LoreEntry],
) -> String {
    let outline_conflict = outline
        .iter()
        .find(|node| contains_any(&node.summary, &["冲突", "危机", "对抗", "矛盾", "敌"]))
        .map(|node| compact_context_line(&node.summary, 96));
    if let Some(conflict) = outline_conflict.filter(|value| !value.is_empty()) {
        return conflict;
    }

    lorebook
        .iter()
        .find(|entry| contains_any(&entry.content, &["冲突", "危机", "对抗", "矛盾", "敌"]))
        .map(|entry| compact_context_line(&entry.content, 96))
        .unwrap_or_else(|| "待明确: 主角欲望、阻力与长期对立面需要在开篇阶段定盘。".to_string())
}

fn infer_structural_boundary(
    lorebook: &[crate::storage::LoreEntry],
    outline: &[crate::storage::OutlineNode],
) -> String {
    let mut boundaries = Vec::new();
    if !lorebook.is_empty() {
        boundaries.push("不得违背已记录 Lorebook 设定");
    }
    if !outline.is_empty() {
        boundaries.push("不得跳过当前大纲承诺的因果推进");
    }
    if boundaries.is_empty() {
        "先保护作者已写正文，不自动改写既有事实。".to_string()
    } else {
        boundaries.join("；")
    }
}

fn project_asset_haystack(
    lorebook: &[crate::storage::LoreEntry],
    outline: &[crate::storage::OutlineNode],
) -> String {
    let mut text = String::new();
    for entry in lorebook.iter().take(20) {
        text.push_str(&entry.keyword);
        text.push('\n');
        text.push_str(&entry.content);
        text.push('\n');
    }
    for node in outline.iter().take(20) {
        text.push_str(&node.chapter_title);
        text.push('\n');
        text.push_str(&node.summary);
        text.push('\n');
    }
    text
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn compact_context_line(text: &str, max_chars: usize) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() <= max_chars {
        cleaned
    } else {
        cleaned.chars().take(max_chars).collect()
    }
}

pub fn assemble_observation_context_with_default_budget(
    task: AgentTask,
    observation: &WriterObservation,
    memory: &WriterMemory,
) -> WritingContextPack {
    let total_budget = task.default_budget();
    assemble_observation_context(task, observation, memory, total_budget)
}

fn non_empty(text: String) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn char_window(text: &str, start: usize, max_chars: usize) -> String {
    let remaining = text.chars().skip(start).collect::<String>();
    truncate_to_budget(&remaining, max_chars).0
}

fn build_canon_slice(
    observation: &WriterObservation,
    memory: &WriterMemory,
    relevance: &WritingRelevance,
    open_promises: &[PlotPromiseSummary],
) -> String {
    let mut lines = Vec::new();
    if let Ok(entities) = memory.list_canon_entities() {
        let candidate_entities = prefilter_canon_candidates(observation, &entities, open_promises);
        let mut scored = candidate_entities
            .into_iter()
            .filter_map(|entity| {
                let score = score_canon_entity(&entity, observation, relevance, open_promises, memory, observation.chapter_title.as_deref().unwrap_or_default());
                if score.score > 0 {
                    Some((score, entity))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        scored.sort_by(|(left, left_entity), (right, right_entity)| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left_entity.name.cmp(&right_entity.name))
        });

        for (score, entity) in scored.into_iter().take(6) {
            lines.push(format_canon_line(&entity, &score.reasons));
        }
    }
    if let Ok(rules) = memory.list_canon_rules(6) {
        for rule in rules {
            lines.push(format!(
                "RULE [{} p{}]: {}",
                rule.category, rule.priority, rule.rule
            ));
        }
    }
    lines.join("\n")
}

fn build_promise_slice(
    observation: &WriterObservation,
    promises: &[PlotPromiseSummary],
    relevance: &WritingRelevance,
    decisions: &[CreativeDecisionSummary],
    memory: &WriterMemory,
) -> String {
    let candidate_promises = prefilter_promise_candidates(observation, promises);
    let chapter_title = observation.chapter_title.as_deref().unwrap_or_default();
    let mut scored = candidate_promises
        .iter()
        .map(|promise| {
            (
                score_promise(promise, observation, relevance, decisions, memory, chapter_title),
                promise,
            )
        })
        .filter(|(score, _)| score.score > 0)
        .collect::<Vec<_>>();
    scored.sort_by(|(left, left_promise), (right, right_promise)| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right_promise.priority.cmp(&left_promise.priority))
            .then_with(|| left_promise.title.cmp(&right_promise.title))
    });

    scored
        .into_iter()
        .take(6)
        .map(|(score, promise)| format_promise_line(promise, &score.reasons))
        .collect::<Vec<_>>()
        .join("\n")
}

fn prefilter_canon_candidates(
    observation: &WriterObservation,
    entities: &[CanonEntitySummary],
    open_promises: &[PlotPromiseSummary],
) -> Vec<CanonEntitySummary> {
    let paragraph = observation.paragraph.as_str();
    let chapter = observation.chapter_title.as_deref().unwrap_or_default();
    let mut selected = entities
        .iter()
        .filter(|entity| {
            paragraph.contains(&entity.name)
                || (!entity.summary.trim().is_empty() && paragraph.contains(entity.summary.as_str()))
                || open_promises
                    .iter()
                    .any(|promise| promise.title.contains(&entity.name) || promise.description.contains(&entity.name))
                || (!chapter.is_empty() && entity.summary.contains(chapter))
        })
        .cloned()
        .collect::<Vec<_>>();
    if selected.len() < 30 {
        for entity in entities.iter().take(30) {
            if selected.iter().any(|existing| existing.name == entity.name) {
                continue;
            }
            selected.push(entity.clone());
            if selected.len() >= 30 {
                break;
            }
        }
    }
    selected
}

fn prefilter_promise_candidates(
    observation: &WriterObservation,
    promises: &[PlotPromiseSummary],
) -> Vec<PlotPromiseSummary> {
    let paragraph = observation.paragraph.as_str();
    let chapter = observation.chapter_title.as_deref().unwrap_or_default();
    let mut selected = promises
        .iter()
        .filter(|promise| {
            paragraph.contains(&promise.title)
                || paragraph.contains(&promise.description)
                || (!chapter.is_empty() && promise.expected_payoff.contains(chapter))
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

fn build_style_slice(memory: &WriterMemory) -> String {
    memory
        .list_style_preferences(6)
        .unwrap_or_default()
        .into_iter()
        .map(|pref| {
            format!(
                "{}: {} (+{} / -{})",
                pref.key, pref.value, pref.accepted_count, pref.rejected_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_decision_slice(decisions: &[CreativeDecisionSummary]) -> String {
    decisions
        .iter()
        .map(|decision| {
            format!(
                "{} [{}]: {}",
                decision.title, decision.decision, decision.rationale
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Truncate text to fit budget, preferring sentence boundaries.
fn truncate_to_budget(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let truncated: String = text.chars().take(max_chars).collect();
    // Try to break at a sentence boundary using char-level scanning
    let chars: Vec<char> = truncated.chars().collect();
    if let Some(last_period) = chars.iter().rposition(|&c| {
        c == '\u{3002}' || c == '\u{FF01}' || c == '\u{FF1F}'  // 。！？
        || c == '.' || c == '!' || c == '?'
    }) {
        (chars[..=last_period].iter().collect(), true)
    } else {
        (truncated, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_agent::observation::{ObservationReason, ObservationSource};

    #[test]
    fn test_ghost_writing_priorities() {
        let p = AgentTask::GhostWriting.source_priorities();
        assert!(p.iter().any(|(s, _, _)| *s == ContextSource::CursorPrefix));
        assert!(p
            .iter()
            .any(|(s, _, _)| *s == ContextSource::ResultFeedback));
        assert!(p.iter().any(|(s, _, _)| *s == ContextSource::NextBeat));
        assert!(p[0].0 == ContextSource::CursorPrefix); // highest priority
    }

    #[test]
    fn test_chapter_gen_includes_all_sources() {
        let p = AgentTask::ChapterGeneration.source_priorities();
        assert!(p
            .iter()
            .any(|(s, _, _)| *s == ContextSource::PreviousChapter));
        assert!(p
            .iter()
            .any(|(s, _, _)| *s == ContextSource::ResultFeedback));
        assert!(p.iter().any(|(s, _, _)| *s == ContextSource::NextBeat));
        assert!(p.iter().any(|(s, _, _)| *s == ContextSource::PromiseSlice));
    }

    #[test]
    fn test_assemble_respects_budget() {
        let provider = |s: ContextSource| -> Option<String> {
            match s {
                ContextSource::CursorPrefix => Some("前缀文本".repeat(100)),
                ContextSource::CursorSuffix => Some("后缀".repeat(50)),
                ContextSource::CanonSlice => Some("canon数据".repeat(80)),
                _ => None,
            }
        };
        let pack = assemble_context_pack(AgentTask::GhostWriting, &provider, 500);
        assert!(pack.total_chars <= 500);
        assert!(!pack.sources.is_empty());
    }

    #[test]
    fn test_required_sources_survive_tight_budget() {
        let provider = |s: ContextSource| -> Option<String> {
            match s {
                ContextSource::CursorPrefix => Some("长前文。".repeat(500)),
                ContextSource::CanonSlice => Some("林墨 weapon=寒影刀".repeat(20)),
                ContextSource::PromiseSlice => Some("玉佩仍未交代下落。".repeat(20)),
                ContextSource::DecisionSlice => Some("保持克制，不用大段自白。".repeat(20)),
                _ => None,
            }
        };
        let pack = assemble_context_pack(AgentTask::GhostWriting, &provider, 620);

        assert!(pack.total_chars <= 620);
        assert!(pack
            .sources
            .iter()
            .any(|source| source.source == ContextSource::CursorPrefix));
        assert!(pack
            .sources
            .iter()
            .any(|source| source.source == ContextSource::CanonSlice));
        assert!(pack
            .sources
            .iter()
            .any(|source| source.source == ContextSource::PromiseSlice));
        assert!(pack
            .budget_report
            .source_reports
            .iter()
            .any(|report| report.source == "CanonSlice" && report.provided > 0));
    }

    #[test]
    fn test_budget_report_records_dropped_sources() {
        let provider = |s: ContextSource| -> Option<String> {
            match s {
                ContextSource::CursorPrefix => Some("长前文。".repeat(200)),
                ContextSource::ChapterMission => Some("本章必须追查玉佩。".repeat(40)),
                ContextSource::AuthorStyle => Some("对白保持克制，用动作暗示情绪。".repeat(40)),
                _ => None,
            }
        };
        let pack = assemble_context_pack(AgentTask::GhostWriting, &provider, 240);

        assert!(pack.total_chars <= 240);
        assert!(pack
            .budget_report
            .source_reports
            .iter()
            .any(|report| report.source == "CursorPrefix" && report.provided > 0));
        let dropped = pack
            .budget_report
            .source_reports
            .iter()
            .find(|report| report.source == "AuthorStyle")
            .expect("budget report should include dropped source with available content");
        assert_eq!(dropped.provided, 0);
        assert!(dropped.reason.contains("dropped"));
        assert!(dropped.truncated);
        assert!(dropped.truncation_reason.is_some());
    }

    #[test]
    fn test_truncate_sentence_boundary() {
        let text = "第一句。第二句。第三句。第四句。";
        let (result, truncated) = truncate_to_budget(text, 8);
        assert!(truncated, "text longer than budget should be truncated");
        // After truncation, the result should be shorter than input
        assert!(result.chars().count() < text.chars().count());
    }

    #[test]
    fn test_task_distinct_budgets() {
        let ghost = AgentTask::GhostWriting.source_priorities();
        let chapter = AgentTask::ChapterGeneration.source_priorities();
        // Chapter generation gets much larger budget
        let ghost_total: usize = ghost.iter().map(|(_, _, b)| b).sum();
        let chapter_total: usize = chapter.iter().map(|(_, _, b)| b).sum();
        assert!(chapter_total > ghost_total * 3);
    }

    #[test]
    fn test_default_task_budgets_match_agent_paths() {
        assert_eq!(AgentTask::GhostWriting.default_budget(), 3_000);
        assert_eq!(AgentTask::InlineRewrite.default_budget(), 4_500);
        assert_eq!(AgentTask::ManualRequest.default_budget(), 4_500);
        assert!(
            AgentTask::ChapterGeneration.default_budget()
                > AgentTask::GhostWriting.default_budget()
        );
    }

    #[test]
    fn test_manual_request_prioritizes_selection_and_ledgers() {
        let p = AgentTask::ManualRequest.source_priorities();
        assert_eq!(p[0].0, ContextSource::SelectedText);
        assert!(p.iter().any(|(s, _, _)| *s == ContextSource::CanonSlice));
        assert!(p.iter().any(|(s, _, _)| *s == ContextSource::PromiseSlice));
        assert!(p.iter().any(|(s, _, _)| *s == ContextSource::DecisionSlice));
        assert!(p.iter().any(|(s, _, _)| *s == ContextSource::AuthorStyle));
    }

    #[test]
    fn test_observation_context_includes_relevant_ledgers() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        memory
            .ensure_story_contract_seed(
                "default",
                "寒影",
                "玄幻",
                "刀客在旧怨中追查玉佩真相。",
                "林墨必须在复仇和守护之间做选择。",
                "不得提前泄露玉佩来源。",
            )
            .unwrap();
        memory
            .ensure_chapter_mission_seed(
                "default",
                "Chapter-1",
                "林墨在旧门前试探屋内人的真实立场。",
                "保留玉佩线索",
                "不要提前揭开玉佩来源",
                "以新的疑问收束。",
                "test",
            )
            .unwrap();
        memory
            .upsert_canon_entity(
                "location",
                "林墨",
                &[],
                "主角",
                &serde_json::json!({ "weapon": "寒影刀" }),
                0.9,
            )
            .unwrap();
        memory
            .add_promise("clue", "玉佩", "张三拿走玉佩", "Chapter-1", "Chapter-5", 3)
            .unwrap();
        memory
            .upsert_style_preference("dialogue", "prefers_subtext", true)
            .unwrap();
        memory
            .record_decision(
                "Chapter-1",
                "林墨不主动解释",
                "accepted",
                &[],
                "保持克制，不用大段自白。",
                &[],
            )
            .unwrap();
        memory
            .record_chapter_result(&crate::writer_agent::memory::ChapterResultSummary {
                id: 0,
                project_id: "default".into(),
                chapter_title: "Chapter-0".into(),
                chapter_revision: "rev-0".into(),
                summary: "上一章林墨确认玉佩仍在张三手里。".into(),
                state_changes: vec!["林墨开始怀疑张三".into()],
                character_progress: vec![],
                new_conflicts: vec!["林墨与张三信任受损".into()],
                new_clues: vec!["玉佩".into()],
                promise_updates: vec![],
                canon_updates: vec![],
                source_ref: "test".into(),
                created_at: 2,
            })
            .unwrap();

        let observation = WriterObservation {
            id: "obs".into(),
            created_at: 1,
            source: ObservationSource::Editor,
            reason: ObservationReason::Idle,
            project_id: "default".into(),
            chapter_title: Some("Chapter-1".into()),
            chapter_revision: Some("rev".into()),
            cursor: None,
            selection: None,
            prefix: "林墨停在门前。".into(),
            suffix: String::new(),
            paragraph: "林墨停在门前。".into(),
            full_text_digest: None,
            editor_dirty: true,
        };
        let pack = assemble_observation_context_with_default_budget(
            AgentTask::GhostWriting,
            &observation,
            &memory,
        );

        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::CursorPrefix));
        assert_eq!(pack.budget_limit, AgentTask::GhostWriting.default_budget());
        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::ProjectBrief));
        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::ChapterMission));
        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::NextBeat));
        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::ResultFeedback));
        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::CanonSlice));
        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::PromiseSlice));
        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::DecisionSlice));
        assert!(pack
            .sources
            .iter()
            .any(|s| s.source == ContextSource::AuthorStyle));
    }

    #[test]
    fn test_seed_story_contract_from_project_assets() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let seeded = seed_story_contract_from_project_assets(
            "novel-a",
            "寒影录",
            &[crate::storage::LoreEntry {
                id: "1".to_string(),
                keyword: "林墨".to_string(),
                content: "林墨来自宗门，惯用寒影刀。".to_string(),
            }],
            &[crate::storage::OutlineNode {
                chapter_title: "第一章".to_string(),
                summary: "林墨卷入宗门危机，发现玉佩线索。".to_string(),
                status: "draft".to_string(),
            }],
            &memory,
        )
        .unwrap();

        assert!(seeded);
        let contract = memory.get_story_contract("novel-a").unwrap().unwrap();
        assert_eq!(contract.title, "寒影录");
        assert_eq!(contract.genre, "玄幻/修仙");
        assert!(contract.reader_promise.contains("林墨"));

        let seeded_again =
            seed_story_contract_from_project_assets("novel-a", "新标题不应覆盖", &[], &[], &memory)
                .unwrap();
        assert!(!seeded_again);
        assert_eq!(
            memory.get_story_contract("novel-a").unwrap().unwrap().title,
            "寒影录"
        );
    }

    #[test]
    fn test_seed_chapter_missions_from_outline() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let outline = vec![
            crate::storage::OutlineNode {
                chapter_title: "第一章".to_string(),
                summary: "林墨发现玉佩线索，引出宗门危机。".to_string(),
                status: "draft".to_string(),
            },
            crate::storage::OutlineNode {
                chapter_title: "第二章".to_string(),
                summary: "林墨与张三产生误会，关系开始紧张。".to_string(),
                status: "draft".to_string(),
            },
        ];

        let seeded = seed_chapter_missions_from_outline("novel-a", &outline, &memory).unwrap();

        assert_eq!(seeded, 2);
        let mission = memory
            .get_chapter_mission("novel-a", "第一章")
            .unwrap()
            .unwrap();
        assert!(mission.mission.contains("玉佩"));
        assert!(mission.must_include.contains("线索"));

        let seeded_again =
            seed_chapter_missions_from_outline("novel-a", &outline, &memory).unwrap();
        assert_eq!(seeded_again, 0);
    }
}
