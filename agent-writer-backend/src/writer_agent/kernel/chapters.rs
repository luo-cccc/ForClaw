//! Chapter result processing, mission calibration, and text analysis helpers.
//! Extracted from kernel.rs.

use crate::writer_agent::kernel::{
    extract_new_canon_entities, extract_plot_promises, sentence_snippet, split_sentences,
};
use crate::writer_agent::memory::ContextBudgetTrace;
use crate::writer_agent::memory::{
    ChapterMissionSummary, ChapterResultSummary, NextBeatSummary, WriterMemory,
};
use crate::writer_agent::observation::WriterObservation;
use crate::writer_agent::operation::WriterOperation;
use crate::writer_agent::proposal::{
    AgentProposal, EvidenceRef, EvidenceSource, ProposalKind, ProposalPriority,
};
use std::collections::HashSet;

pub(crate) fn proposal_id(session_id: &str, counter: u64) -> String {
    format!("prop_{}_{}", session_id, counter)
}

pub(crate) fn chapter_result_from_observation(
    observation: &WriterObservation,
    memory: &WriterMemory,
) -> ChapterResultSummary {
    let text = if observation.prefix.trim().is_empty() {
        observation.paragraph.as_str()
    } else {
        observation.prefix.as_str()
    };
    let sentences = split_sentences(text);
    let summary = chapter_result_summary(&sentences, text);
    let known_names = memory.get_canon_entity_names().unwrap_or_default();
    let known_entities = memory.list_canon_entities().unwrap_or_default();
    let open_promises = memory.get_open_promise_summaries().unwrap_or_default();
    let canon_candidates = extract_new_canon_entities(text, &known_names);
    let promise_candidates = extract_plot_promises(text, observation);
    let character_cues = chapter_result_character_cues(&known_entities);
    let clue_markers = chapter_result_clue_markers(&known_entities, &open_promises);

    let chapter_title = observation
        .chapter_title
        .clone()
        .unwrap_or_else(|| "current chapter".to_string());
    let chapter_revision = observation
        .chapter_revision
        .clone()
        .or_else(|| observation.full_text_digest.clone())
        .unwrap_or_default();

    ChapterResultSummary {
        id: 0,
        project_id: observation.project_id.clone(),
        chapter_title: chapter_title.clone(),
        chapter_revision: chapter_revision.clone(),
        summary,
        state_changes: extract_result_lines(
            &sentences,
            &[
                "决定", "选择", "发现", "得知", "确认", "失去", "拿走", "交给", "离开",
            ],
            4,
        ),
        character_progress: extract_result_lines_owned(&sentences, &character_cues, 4),
        new_conflicts: extract_result_lines(
            &sentences,
            &["冲突", "对抗", "争执", "怀疑", "背叛", "危机", "敌", "杀意"],
            4,
        ),
        new_clues: extract_unique_marker_values(text, &clue_markers),
        promise_updates: promise_candidates
            .into_iter()
            .take(4)
            .map(|promise| format!("{}: {}", promise.title, promise.description))
            .collect(),
        canon_updates: canon_candidates
            .into_iter()
            .take(4)
            .map(|entity| format!("{} [{}]: {}", entity.name, entity.kind, entity.summary))
            .collect(),
        source_ref: format!("chapter_save:{}:{}", chapter_title, chapter_revision),
        created_at: observation.created_at,
    }
}

pub(crate) fn derive_next_beat(
    active_chapter: Option<&str>,
    active_mission: Option<&ChapterMissionSummary>,
    recent_results: &[ChapterResultSummary],
    open_promises: &[super::memory::PlotPromiseSummary],
) -> Option<NextBeatSummary> {
    let latest_result = recent_results.first()?;
    let chapter_title = active_chapter
        .or_else(|| active_mission.map(|mission| mission.chapter_title.as_str()))
        .unwrap_or(latest_result.chapter_title.as_str())
        .to_string();

    let mut carryovers = Vec::new();
    push_unique_lines(
        &mut carryovers,
        latest_result.new_conflicts.iter().cloned(),
        3,
    );
    push_unique_lines(
        &mut carryovers,
        latest_result.promise_updates.iter().cloned(),
        5,
    );
    push_unique_lines(
        &mut carryovers,
        latest_result
            .new_clues
            .iter()
            .map(|clue| format!("继续处理线索: {}", clue)),
        6,
    );
    push_unique_lines(
        &mut carryovers,
        open_promises
            .iter()
            .take(3)
            .map(|promise| format!("未回收伏笔: {}", promise.title)),
        7,
    );

    let mut blockers = Vec::new();
    if let Some(mission) = active_mission {
        match mission.status.as_str() {
            "drifted" => blockers.push(format!("上一轮任务偏离: {}", mission.must_not)),
            "needs_review" => blockers.push("上一轮任务与结果匹配度低，需要人工复核。".to_string()),
            "active" => blockers.push("上一轮任务只完成部分，需要继续接住。".to_string()),
            "blocked" => blockers.push("本章任务被阻塞，需要先解除阻塞或改写任务。".to_string()),
            _ => {}
        }
    }

    let goal = next_beat_goal(active_mission, latest_result, &carryovers);
    let source_refs = [latest_result.source_ref.clone()]
        .into_iter()
        .chain(active_mission.map(|mission| mission.source_ref.clone()))
        .filter(|source| !source.trim().is_empty())
        .collect::<Vec<_>>();

    let next = NextBeatSummary {
        chapter_title,
        goal,
        carryovers,
        blockers,
        source_refs,
    };
    if next.is_empty() {
        None
    } else {
        Some(next)
    }
}

pub(crate) fn next_beat_goal(
    active_mission: Option<&ChapterMissionSummary>,
    latest_result: &ChapterResultSummary,
    carryovers: &[String],
) -> String {
    if let Some(mission) = active_mission {
        if mission.status == "drifted" {
            return format!("先修正任务偏离，再回到本章任务: {}", mission.mission);
        }
        if mission.status == "needs_review" {
            return format!("复核本章任务是否还成立: {}", mission.mission);
        }
        if !matches!(mission.status.as_str(), "completed" | "retired")
            && !mission.mission.trim().is_empty()
        {
            return mission.mission.clone();
        }
    }

    if let Some(conflict) = latest_result.new_conflicts.first() {
        format!("承接上一章冲突后果: {}", conflict)
    } else if let Some(carryover) = carryovers.first() {
        carryover.clone()
    } else if let Some(clue) = latest_result.new_clues.first() {
        format!("让线索产生新的选择或代价: {}", clue)
    } else {
        format!("承接上一章结果: {}", latest_result.summary)
    }
}

pub(crate) fn push_unique_lines<I>(target: &mut Vec<String>, lines: I, max_len: usize)
where
    I: IntoIterator<Item = String>,
{
    let mut seen = target.iter().cloned().collect::<HashSet<_>>();
    for line in lines {
        let line = line.trim().to_string();
        if line.is_empty() || !seen.insert(line.clone()) {
            continue;
        }
        target.push(line);
        if target.len() >= max_len {
            break;
        }
    }
}

pub(crate) fn calibrated_mission_status(
    mission: &ChapterMissionSummary,
    result: &ChapterResultSummary,
) -> String {
    if matches!(mission.status.as_str(), "blocked" | "retired") {
        return mission.status.clone();
    }

    let haystack = mission_result_haystack(result);
    let violation_haystack = mission_result_violation_haystack(result);
    let must_not_hit = cue_violation_hit_score(&mission.must_not, &violation_haystack) > 0;
    if must_not_hit {
        return "drifted".to_string();
    }

    let must_include_score = cue_hit_score(&mission.must_include, &haystack);
    let expected_ending_score = cue_hit_score(&mission.expected_ending, &haystack);
    let mission_score = cue_hit_score(&mission.mission, &haystack);

    if must_include_score > 0 && expected_ending_score > 0 {
        "completed".to_string()
    } else if must_include_score > 0 || expected_ending_score > 0 || mission_score > 1 {
        "active".to_string()
    } else {
        "needs_review".to_string()
    }
}

pub(crate) fn chapter_mission_result_proposals(
    observation: &WriterObservation,
    result: &ChapterResultSummary,
    memory: &WriterMemory,
    observation_id: &str,
    proposal_counter: &mut u64,
    session_id: &str,
) -> Vec<AgentProposal> {
    let Some(chapter_title) = observation.chapter_title.as_deref() else {
        return Vec::new();
    };
    let Ok(Some(mission)) = memory.get_chapter_mission(&observation.project_id, chapter_title)
    else {
        return Vec::new();
    };
    let haystack = mission_result_haystack(result);
    let violation_haystack = mission_result_violation_haystack(result);
    if cue_violation_hit_score(&mission.must_not, &violation_haystack) > 0 {
        return Vec::new();
    }
    if matches!(
        mission.status.as_str(),
        "completed" | "drifted" | "blocked" | "retired"
    ) {
        return Vec::new();
    }
    if mission.must_include.trim().is_empty() || cue_hit_score(&mission.must_include, &haystack) > 0
    {
        return Vec::new();
    }

    let proposal = AgentProposal {
        id: proposal_id(session_id, *proposal_counter),
        observation_id: observation_id.to_string(),
        kind: ProposalKind::ChapterMission,
        priority: ProposalPriority::Normal,
        target: observation.cursor.clone(),
        preview: format!(
            "章节任务缺口: {} 保存后仍未体现必保事项「{}」",
            chapter_title, mission.must_include
        ),
        operations: vec![WriterOperation::TextAnnotate {
            chapter: chapter_title.to_string(),
            from: observation
                .cursor
                .as_ref()
                .map(|range| range.from)
                .unwrap_or(0),
            to: observation
                .cursor
                .as_ref()
                .map(|range| range.to.max(range.from + 1))
                .unwrap_or(1),
            message: format!("本章必保事项尚未兑现：{}", mission.must_include),
            severity: super::operation::AnnotationSeverity::Warning,
        }],
        rationale: format!(
            "Chapter save result did not satisfy mission must_include. Status candidate: {}.",
            calibrated_mission_status(&mission, result)
        ),
        evidence: vec![
            EvidenceRef {
                source: EvidenceSource::ChapterMission,
                reference: format!("{}:must_include", chapter_title),
                snippet: mission.must_include.clone(),
            },
            EvidenceRef {
                source: EvidenceSource::ChapterText,
                reference: result.source_ref.clone(),
                snippet: result.summary.clone(),
            },
        ],
        risks: vec![
            "Ignoring this can let the chapter end without paying its assigned job.".into(),
        ],
        alternatives: vec![],
        confidence: 0.74,
        expires_at: None,
    };
    *proposal_counter += 1;
    vec![proposal]
}

pub(crate) fn mission_result_haystack(result: &ChapterResultSummary) -> String {
    [
        vec![result.summary.clone()],
        result.state_changes.clone(),
        result.character_progress.clone(),
        result.new_conflicts.clone(),
        result.new_clues.clone(),
        result.promise_updates.clone(),
        result.canon_updates.clone(),
    ]
    .concat()
    .join("\n")
}

pub(crate) fn mission_result_violation_haystack(result: &ChapterResultSummary) -> String {
    [
        vec![result.summary.clone()],
        result.state_changes.clone(),
        result.character_progress.clone(),
        result.new_conflicts.clone(),
        result.promise_updates.clone(),
        result.canon_updates.clone(),
    ]
    .concat()
    .join("\n")
}

pub(crate) fn cue_hit_score(cues: &str, haystack: &str) -> usize {
    mission_keywords(cues)
        .into_iter()
        .filter(|cue| haystack.contains(cue))
        .count()
}

pub(crate) fn cue_violation_hit_score(cues: &str, haystack: &str) -> usize {
    mission_keywords(cues)
        .into_iter()
        .filter(|cue| cue_occurs_without_negation(haystack, cue))
        .count()
}

pub(crate) fn cue_occurs_without_negation(haystack: &str, cue: &str) -> bool {
    let mut search_from = 0usize;
    while let Some(relative) = haystack[search_from..].find(cue) {
        let byte_pos = search_from + relative;
        let char_pos = haystack[..byte_pos].chars().count();
        if !mission_context_negated_or_deferred_before(haystack, char_pos) {
            return true;
        }
        search_from = byte_pos + cue.len();
        if search_from >= haystack.len() {
            break;
        }
    }
    false
}

pub(crate) fn mission_context_negated_or_deferred_before(text: &str, match_from: usize) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    let start = match_from.saturating_sub(10);
    let context = chars[start..match_from.min(chars.len())]
        .iter()
        .collect::<String>();
    [
        "没有",
        "并未",
        "未曾",
        "尚未",
        "还没",
        "不会",
        "不能",
        "不该",
        "不肯",
        "拒绝",
        "暂不",
        "避免",
        "仍没有",
        "没有真正",
        "并没有",
    ]
    .iter()
    .any(|cue| context.contains(cue))
}

pub(crate) fn mission_keywords(text: &str) -> Vec<String> {
    let stopwords = [
        "保持", "推进", "不要", "不得", "本章", "当前", "目标", "任务", "需要", "后续", "解释",
        "收束", "新的", "明确", "状态", "变化", "线索", "冲突", "角色",
    ];
    let mut keywords = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            current.push(ch);
        } else {
            push_mission_keyword(&mut keywords, &current, &stopwords);
            current.clear();
        }
    }
    push_mission_keyword(&mut keywords, &current, &stopwords);
    keywords.sort();
    keywords.dedup();
    keywords
}

pub(crate) fn push_mission_keyword(keywords: &mut Vec<String>, raw: &str, stopwords: &[&str]) {
    let raw = raw.trim();
    if raw.chars().count() < 2 || stopwords.contains(&raw) {
        return;
    }
    if raw.chars().count() <= 6 {
        keywords.push(raw.to_string());
    }

    for marker in [
        "玉佩",
        "寒影刀",
        "密信",
        "钥匙",
        "令牌",
        "真相",
        "秘密",
        "疑问",
        "下落",
        "来源",
        "禁地",
        "信任",
        "怀疑",
    ] {
        if raw.contains(marker) {
            keywords.push(marker.to_string());
        }
    }
}

pub(crate) fn chapter_result_character_cues(
    entities: &[super::memory::CanonEntitySummary],
) -> Vec<String> {
    let mut cues = entities
        .iter()
        .filter(|entity| {
            let kind = entity.kind.to_ascii_lowercase();
            kind.contains("character") || kind.contains("person") || kind.contains("角色")
        })
        .map(|entity| entity.name.clone())
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>();
    if cues.is_empty() {
        cues.extend(
            ["主角", "少年", "少女", "师父"]
                .into_iter()
                .map(String::from),
        );
    }
    cues
}

pub(crate) fn chapter_result_clue_markers(
    entities: &[super::memory::CanonEntitySummary],
    promises: &[super::memory::PlotPromiseSummary],
) -> Vec<String> {
    let mut markers = [
        "玉佩",
        "寒影刀",
        "密信",
        "钥匙",
        "令牌",
        "真相",
        "秘密",
        "线索",
        "下落",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    markers.extend(
        entities
            .iter()
            .filter(|entity| {
                let kind = entity.kind.to_ascii_lowercase();
                kind.contains("object")
                    || kind.contains("place")
                    || kind.contains("rule")
                    || kind.contains("物")
                    || kind.contains("地点")
            })
            .map(|entity| entity.name.clone()),
    );
    markers.extend(promises.iter().map(|promise| promise.title.clone()));
    markers
        .into_iter()
        .map(|marker| marker.trim().to_string())
        .filter(|marker| !marker.is_empty())
        .collect()
}

pub(crate) fn chapter_result_summary(sentences: &[String], text: &str) -> String {
    let mut parts = Vec::new();
    if let Some(first) = sentences.first() {
        parts.push(sentence_snippet(first, 120));
    }
    if let Some(last) = sentences.last() {
        let last = sentence_snippet(last, 120);
        if parts.first().map(|first| first != &last).unwrap_or(true) {
            parts.push(last);
        }
    }
    if parts.is_empty() {
        sentence_snippet(text, 180)
    } else {
        parts.join(" / ")
    }
}

pub(crate) fn extract_result_lines(
    sentences: &[String],
    cues: &[&str],
    limit: usize,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for sentence in sentences {
        if !contains_any(sentence, cues) {
            continue;
        }
        let line = sentence_snippet(sentence, 120);
        if seen.insert(line.clone()) {
            lines.push(line);
        }
        if lines.len() >= limit {
            break;
        }
    }
    lines
}

pub(crate) fn extract_result_lines_owned(
    sentences: &[String],
    cues: &[String],
    limit: usize,
) -> Vec<String> {
    let refs = cues.iter().map(String::as_str).collect::<Vec<_>>();
    extract_result_lines(sentences, &refs, limit)
}

pub(crate) fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

pub(crate) fn extract_unique_marker_values(text: &str, markers: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut values = Vec::new();
    for marker in markers {
        if text.contains(marker) && seen.insert(marker.clone()) {
            values.push(marker.clone());
        }
        if values.len() >= 8 {
            break;
        }
    }
    values
}

pub(crate) fn snippet(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

pub(crate) fn proposal_trace_summary(
    proposal: &AgentProposal,
    state: &str,
    context_budget: Option<ContextBudgetTrace>,
) -> super::memory::ProposalTraceSummary {
    super::memory::ProposalTraceSummary {
        id: proposal.id.clone(),
        observation_id: proposal.observation_id.clone(),
        kind: format!("{:?}", proposal.kind),
        priority: format!("{:?}", proposal.priority),
        state: state.to_string(),
        confidence: proposal.confidence,
        preview_snippet: snippet(&proposal.preview, 120),
        evidence: proposal.evidence.clone(),
        context_budget,
        expires_at: proposal.expires_at,
    }
}
