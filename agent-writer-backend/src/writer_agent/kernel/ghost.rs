//! Ghost proposal helpers for WriterAgentKernel.

use crate::writer_agent::context::{ContextSource, WritingContextPack};
use crate::writer_agent::intent::WritingIntent;
use crate::writer_agent::memory::WriterMemory;
use crate::writer_agent::observation::WriterObservation;
use crate::writer_agent::operation::WriterOperation;
use crate::writer_agent::proposal::{EvidenceRef, EvidenceSource, ProposalAlternative};

pub(crate) fn draft_continuation(
    intent: &WritingIntent,
    observation: &WriterObservation,
    context_pack: &WritingContextPack,
) -> String {
    let paragraph = observation.paragraph.trim();
    let lead = if paragraph.ends_with('。')
        || paragraph.ends_with('！')
        || paragraph.ends_with('？')
        || paragraph.ends_with('.')
        || paragraph.ends_with('!')
        || paragraph.ends_with('?')
    {
        "\n"
    } else {
        ""
    };

    let canon_hint = context_pack
        .sources
        .iter()
        .find(|source| source.source == ContextSource::CanonSlice)
        .and_then(|source| source.content.lines().next())
        .unwrap_or("");
    let promise_hint = context_pack
        .sources
        .iter()
        .find(|source| source.source == ContextSource::PromiseSlice)
        .and_then(|source| source.content.lines().next())
        .unwrap_or("");

    let text = if !promise_hint.is_empty() {
        "他忽然想起那件还没交代清楚的旧事，原本要出口的话在舌尖停住了。"
    } else if canon_hint.contains("weapon") || canon_hint.contains("武器") {
        "他没有急着开口，只让手指重新落回熟悉的兵器旁，像是在确认自己仍握着选择。"
    } else {
        match intent {
            WritingIntent::Dialogue => "他没有立刻回答，只把真正想说的话压在喉咙后面。",
            WritingIntent::Action => "下一瞬，他侧身避开逼近的锋芒，顺势把局面逼向更窄的角落。",
            WritingIntent::ConflictEscalation => {
                "偏在这时，门外传来第三个人的脚步声，把所有尚未出口的话都截断了。"
            }
            WritingIntent::Description => {
                "风从缝隙里钻进来，带着潮湿的冷意，让这片沉默显得更不安稳。"
            }
            _ => "他停了半息，终于做出那个无法再撤回的决定。",
        }
    };

    format!("{lead}{text}")
}

pub(crate) fn ghost_alternatives(
    intent: &WritingIntent,
    observation: &WriterObservation,
    context_pack: &WritingContextPack,
    chapter: &str,
    insert_at: usize,
    revision: &str,
) -> Vec<ProposalAlternative> {
    let candidates = ghost_candidate_texts(intent, observation, context_pack);
    let labels = ghost_candidate_labels(intent);
    let branch_evidence = per_branch_evidence(intent, context_pack);
    candidates
        .into_iter()
        .enumerate()
        .map(|(idx, preview)| {
            let id = ["a", "b", "c"].get(idx).unwrap_or(&"x").to_string();
            ProposalAlternative {
                id: id.clone(),
                label: labels[idx].to_string(),
                operation: Some(WriterOperation::TextInsert {
                    chapter: chapter.to_string(),
                    at: insert_at,
                    text: preview.clone(),
                    revision: revision.to_string(),
                }),
                rationale: format!("multi-ghost branch {}", id.to_ascii_uppercase()),
                evidence: branch_evidence[idx].clone(),
                preview,
            }
        })
        .collect()
}

fn per_branch_evidence(
    intent: &WritingIntent,
    context_pack: &WritingContextPack,
) -> [Vec<EvidenceRef>; 3] {
    let mission_snippet = context_pack
        .sources
        .iter()
        .find(|s| s.source == ContextSource::ChapterMission)
        .map(|s| crate::writer_agent::kernel::snippet(&s.content, 80))
        .unwrap_or_default();
    let canon_snippet = context_pack
        .sources
        .iter()
        .find(|s| s.source == ContextSource::CanonSlice)
        .and_then(|s| s.content.lines().next().map(|l| l.to_string()))
        .unwrap_or_default();
    let promise_snippet = context_pack
        .sources
        .iter()
        .find(|s| s.source == ContextSource::PromiseSlice)
        .and_then(|s| s.content.lines().next().map(|l| l.to_string()))
        .unwrap_or_default();

    let a_evidence = if !canon_snippet.is_empty() {
        vec![EvidenceRef {
            source: EvidenceSource::Canon,
            reference: "current-canon".to_string(),
            snippet: canon_snippet.clone(),
        }]
    } else if !mission_snippet.is_empty() {
        vec![EvidenceRef {
            source: EvidenceSource::ChapterMission,
            reference: "active-mission".to_string(),
            snippet: mission_snippet.clone(),
        }]
    } else {
        vec![]
    };

    let b_evidence = if !promise_snippet.is_empty() {
        vec![EvidenceRef {
            source: EvidenceSource::PromiseLedger,
            reference: "open-promise".to_string(),
            snippet: promise_snippet.clone(),
        }]
    } else if !mission_snippet.is_empty() {
        vec![EvidenceRef {
            source: EvidenceSource::ChapterMission,
            reference: "active-mission".to_string(),
            snippet: mission_snippet.clone(),
        }]
    } else {
        vec![]
    };

    let c_evidence = if matches!(
        intent,
        WritingIntent::ConflictEscalation | WritingIntent::Dialogue
    ) && !mission_snippet.is_empty()
    {
        vec![EvidenceRef {
            source: EvidenceSource::ChapterMission,
            reference: "active-mission".to_string(),
            snippet: mission_snippet.clone(),
        }]
    } else if !canon_snippet.is_empty() {
        vec![EvidenceRef {
            source: EvidenceSource::Canon,
            reference: "current-canon".to_string(),
            snippet: canon_snippet.clone(),
        }]
    } else {
        vec![]
    };

    [a_evidence, b_evidence, c_evidence]
}

fn ghost_candidate_texts(
    intent: &WritingIntent,
    observation: &WriterObservation,
    context_pack: &WritingContextPack,
) -> [String; 3] {
    let base = draft_continuation(intent, observation, context_pack);
    let promise_hint = context_pack
        .sources
        .iter()
        .find(|source| source.source == ContextSource::PromiseSlice)
        .and_then(|source| source.content.lines().next())
        .unwrap_or("");
    let canon_hint = context_pack
        .sources
        .iter()
        .find(|source| source.source == ContextSource::CanonSlice)
        .and_then(|source| source.content.lines().next())
        .unwrap_or("");

    let branch_b = if !promise_hint.is_empty() {
        "他没有继续逼问，只把那件悬而未决的旧事重新压回心底，等对方先露出破绽。"
    } else {
        match intent {
            WritingIntent::Dialogue => "他垂下眼，像是随口一问：“你刚才避开的，究竟是哪一句？”",
            WritingIntent::Action => "他故意慢了半拍，让对方以为自己占了先机，再突然切进空门。",
            WritingIntent::ConflictEscalation => {
                "他还没来得及判断局势，屋内的灯先灭了，黑暗把所有退路一并吞没。"
            }
            WritingIntent::Description => {
                "潮气沿着墙根蔓延，旧木与灰尘的味道混在一起，像某种迟迟不肯散去的警告。"
            }
            _ => "他没有立刻推进，只把目光移向最安静的那个人，等一个真正的答案。",
        }
    };

    let branch_c = if canon_hint.contains("weapon") || canon_hint.contains("武器") {
        "他松开那句差点出口的话，先确认掌心熟悉的重量仍在，才重新抬眼看向对方。"
    } else {
        match intent {
            WritingIntent::Dialogue => {
                "那句话到了嘴边又被他咽回去，只剩一个短促的笑，听不出是承认还是挑衅。"
            }
            WritingIntent::Action => {
                "可就在他发力之前，身后传来一声轻响，迫使他把所有动作硬生生收住。"
            }
            WritingIntent::ConflictEscalation => {
                "更糟的是，来人没有藏脚步，仿佛正等着他们意识到自己已经无处可躲。"
            }
            WritingIntent::Description => {
                "远处的声响被夜色压得很低，低到像是从每个人心里慢慢渗出来的。"
            }
            _ => "他终于意识到，真正该被追问的不是眼前这句话，而是此前一直没人敢提的沉默。",
        }
    };

    [base, branch_b.to_string(), branch_c.to_string()]
}

fn ghost_candidate_labels(intent: &WritingIntent) -> [&'static str; 3] {
    match intent {
        WritingIntent::Dialogue => ["A 直接表态", "B 言语试探", "C 压住情绪"],
        WritingIntent::Action => ["A 快节奏", "B 诱敌试探", "C 外部打断"],
        WritingIntent::ConflictEscalation => ["A 顺势加压", "B 黑暗反转", "C 来人压迫"],
        WritingIntent::Description => ["A 氛围推进", "B 感官细化", "C 情绪映射"],
        _ => ["A 顺势推进", "B 关系试探", "C 伏笔回扣"],
    }
}

pub(crate) fn sanitize_continuation(text: &str) -> String {
    text.trim()
        .trim_matches('`')
        .trim()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(260)
        .collect()
}

pub(crate) fn context_pack_evidence(
    pack: &WritingContextPack,
    observation: &WriterObservation,
) -> Vec<EvidenceRef> {
    let mut evidence = Vec::new();
    for source in &pack.sources {
        let evidence_source = match source.source {
            ContextSource::CursorPrefix
            | ContextSource::CursorSuffix
            | ContextSource::SelectedText => EvidenceSource::ChapterText,
            ContextSource::CanonSlice => EvidenceSource::Canon,
            ContextSource::PromiseSlice => EvidenceSource::PromiseLedger,
            ContextSource::ProjectBrief | ContextSource::BookState => EvidenceSource::StoryContract,
            ContextSource::ChapterMission => EvidenceSource::ChapterMission,
            ContextSource::DecisionSlice => EvidenceSource::AuthorFeedback,
            ContextSource::AuthorStyle => EvidenceSource::StyleLedger,
            ContextSource::OutlineSlice
            | ContextSource::ArcSnapshot
            | ContextSource::VolumeSnapshot => EvidenceSource::Outline,
            ContextSource::ResultFeedback => EvidenceSource::ChapterText,
            ContextSource::StoryImpactRadius => EvidenceSource::StoryImpactRadius,
            _ => EvidenceSource::ChapterText,
        };
        evidence.push(EvidenceRef {
            source: evidence_source,
            reference: format!("{:?}", source.source),
            snippet: source.content.chars().take(140).collect(),
        });
    }

    if evidence.is_empty() {
        evidence.push(EvidenceRef {
            source: EvidenceSource::ChapterText,
            reference: observation
                .chapter_title
                .clone()
                .unwrap_or_else(|| "current chapter".into()),
            snippet: observation.paragraph.chars().take(120).collect(),
        });
    }

    evidence
}

/// Read accepted style preferences from memory and annotate ghost alternatives.
/// When an alternative's rationale aligns with an accepted style key, it receives a
/// style-confidence boost marker that downstream proposal scoring can consume.
pub(crate) fn ghost_consume_style_preferences(
    memory: &WriterMemory,
    alternatives: &mut [ProposalAlternative],
) -> Vec<EvidenceRef> {
    let prefs = match memory.list_style_preferences(20) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    if prefs.is_empty() || alternatives.len() <= 1 {
        return Vec::new();
    }
    let accepted_preferences = prefs
        .iter()
        .filter(|p| p.accepted_count > 0)
        .collect::<Vec<_>>();
    if accepted_preferences.is_empty() {
        return Vec::new();
    }

    let mut rankings = vec![(0i32, Vec::<String>::new()); alternatives.len()];
    let mut style_evidence = Vec::new();

    for preference in accepted_preferences {
        let mut matched_any = false;
        for (index, alternative) in alternatives.iter().enumerate() {
            let score =
                ghost_style_alignment_score(&preference.key, &preference.value, alternative);
            if score > 0 {
                rankings[index].0 += score;
                rankings[index].1.push(preference.key.clone());
                matched_any = true;
            }
        }
        if matched_any {
            style_evidence.push(EvidenceRef {
                source: EvidenceSource::StyleLedger,
                reference: preference.key.clone(),
                snippet: preference.value.clone(),
            });
        }
    }

    if style_evidence.is_empty() {
        return Vec::new();
    }

    let original = alternatives.to_vec();
    let mut order = (0..alternatives.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        rankings[*right]
            .0
            .cmp(&rankings[*left].0)
            .then_with(|| left.cmp(right))
    });

    for (destination, source_index) in order.into_iter().enumerate() {
        let mut alternative = original[source_index].clone();
        let (score, preference_keys) = &rankings[source_index];
        if *score > 0 {
            let matched_keys = dedupe_keys(preference_keys)
                .into_iter()
                .take(2)
                .collect::<Vec<_>>();
            let boost_marker = format!(" style_pref:{} (+{})", matched_keys.join(","), score);
            if !alternative.rationale.contains("style_pref:") {
                alternative.rationale.push_str(&boost_marker);
            }
        }
        alternatives[destination] = alternative;
    }

    style_evidence.truncate(3);
    style_evidence
}

fn ghost_style_alignment_score(key: &str, value: &str, alternative: &ProposalAlternative) -> i32 {
    let preference = format!("{} {}", key, value).to_lowercase();
    let alternative_text = format!("{} {}", alternative.label, alternative.preview).to_lowercase();
    let preview_len = alternative.preview.chars().count();
    let mut score = 0;

    if contains_any(
        &preference,
        &[
            "dialogue.subtext",
            "dialogue_subtext",
            "prefers_subtext",
            "subtext",
            "潜台词",
            "留白",
            "解释情绪",
        ],
    ) {
        score += score_hits(
            &alternative_text,
            &[
                "咽回去",
                "听不出",
                "像是",
                "没有立刻",
                "压在",
                "停住",
                "短促的笑",
                "挑衅",
                "露出破绽",
            ],
            2,
        );
        if alternative.preview.contains('“') || alternative.preview.contains('"') {
            score += 1;
        }
        score -= score_hits(&alternative_text, &["直接表态", "完整解释", "真正想说"], 1);
    }

    if contains_any(
        &preference,
        &[
            "prose.sentence_length",
            "sentence_length",
            "sentence length",
            "短句",
            "长句",
            "short sentence",
            "long sentence",
        ],
    ) {
        if contains_any(
            &preference,
            &["短句", "short", "利落", "克制", "简洁", "tight"],
        ) {
            score += if preview_len <= 32 {
                2
            } else if preview_len <= 42 {
                1
            } else {
                0
            };
        }
        if contains_any(&preference, &["长句", "long", "铺陈", "舒展", "elongated"]) {
            score += if preview_len >= 42 {
                2
            } else if preview_len >= 34 {
                1
            } else {
                0
            };
        }
    }

    if contains_any(
        &preference,
        &[
            "description.sensory_detail",
            "sensory_detail",
            "sensory",
            "描写",
            "感官",
            "气味",
            "触感",
            "画面",
        ],
    ) {
        score += score_hits(
            &alternative_text,
            &["风", "冷意", "潮气", "味道", "声响", "夜色", "沉默", "缝隙"],
            1,
        );
    }

    if contains_any(
        &preference,
        &[
            "structure.hook",
            "chapter_hook",
            "hook",
            "cliffhanger",
            "悬念",
            "钩子",
            "章尾",
            "转折",
        ],
    ) {
        score += score_hits(
            &alternative_text,
            &[
                "脚步声",
                "灯先灭了",
                "无处可躲",
                "更糟",
                "截断",
                "黑暗",
                "来人",
            ],
            2,
        );
    }

    if contains_any(
        &preference,
        &[
            "action.clarity",
            "action",
            "combat",
            "动作",
            "打斗",
            "追逐",
            "交锋",
        ],
    ) {
        score += score_hits(
            &alternative_text,
            &["侧身", "避开", "切进", "先机", "发力", "逼近", "收住"],
            1,
        );
    }

    if contains_any(
        &preference,
        &[
            "tone.voice",
            "tone",
            "voice",
            "克制",
            "冷峻",
            "轻松",
            "幽默",
            "风格",
        ],
    ) && contains_any(
        &preference,
        &["克制", "冷峻", "留白", "subtle", "restrained"],
    ) {
        score += score_hits(
            &alternative_text,
            &["没有立刻", "压在", "咽回去", "停住", "沉默", "短促"],
            1,
        );
    }

    score.max(0)
}

fn score_hits(haystack: &str, needles: &[&str], weight: i32) -> i32 {
    needles
        .iter()
        .filter(|needle| haystack.contains(**needle))
        .count() as i32
        * weight
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn dedupe_keys(keys: &[String]) -> Vec<String> {
    let mut unique = Vec::new();
    for key in keys {
        if !unique.iter().any(|existing| existing == key) {
            unique.push(key.clone());
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_agent::context::{
        AgentTask, ContextBudgetReport, ContextExcerpt, SourceReport,
    };
    use crate::writer_agent::observation::{ObservationReason, ObservationSource, TextRange};

    fn observation(paragraph: &str) -> WriterObservation {
        WriterObservation {
            id: "obs-1".to_string(),
            created_at: 1,
            source: ObservationSource::Editor,
            reason: ObservationReason::Idle,
            project_id: "default".to_string(),
            chapter_title: Some("Chapter-1".to_string()),
            chapter_revision: Some("rev".to_string()),
            cursor: Some(TextRange { from: 10, to: 10 }),
            selection: None,
            prefix: paragraph.to_string(),
            suffix: String::new(),
            paragraph: paragraph.to_string(),
            full_text_digest: None,
            editor_dirty: true,
        }
    }

    fn context_pack(sources: Vec<(ContextSource, &str)>) -> WritingContextPack {
        let excerpts = sources
            .into_iter()
            .map(|(source, content)| ContextExcerpt {
                source,
                content: content.to_string(),
                char_count: content.chars().count(),
                truncated: false,
                priority: 1,
                evidence_ref: None,
            })
            .collect::<Vec<_>>();
        let total_chars = excerpts.iter().map(|source| source.char_count).sum();
        WritingContextPack {
            task: AgentTask::GhostWriting,
            sources: excerpts,
            total_chars,
            budget_limit: 3000,
            budget_report: ContextBudgetReport {
                total_budget: 3000,
                used: total_chars,
                wasted: 3000usize.saturating_sub(total_chars),
                source_reports: vec![SourceReport {
                    source: "test".to_string(),
                    requested: total_chars,
                    provided: total_chars,
                    truncated: false,
                    reason: "test".to_string(),
                    truncation_reason: None,
                }],
            },
        }
    }

    #[test]
    fn draft_continuation_leads_after_sentence_end() {
        let observation = observation("他说完了。");
        let pack = context_pack(Vec::new());

        let continuation = draft_continuation(&WritingIntent::Dialogue, &observation, &pack);

        assert!(continuation.starts_with('\n'));
        assert!(continuation.contains("没有立刻回答"));
    }

    #[test]
    fn ghost_alternatives_build_typed_insert_operations() {
        let observation = observation("他抬头");
        let pack = context_pack(Vec::new());

        let alternatives = ghost_alternatives(
            &WritingIntent::Action,
            &observation,
            &pack,
            "Chapter-1",
            12,
            "rev-1",
        );

        assert_eq!(alternatives.len(), 3);
        assert_eq!(alternatives[0].label, "A 快节奏");
        assert!(matches!(
            alternatives[0].operation,
            Some(WriterOperation::TextInsert { at: 12, .. })
        ));
    }

    #[test]
    fn sanitize_continuation_trims_fences_and_blank_lines() {
        let sanitized = sanitize_continuation("  ```\n\n  第一行\n\n  第二行  \n```  ");

        assert_eq!(sanitized, "第一行\n第二行");
    }

    #[test]
    fn context_pack_evidence_maps_sources_and_falls_back() {
        let observation = observation("这是当前段落");
        let pack = context_pack(vec![
            (ContextSource::CanonSlice, "canon detail"),
            (ContextSource::PromiseSlice, "promise detail"),
        ]);

        let evidence = context_pack_evidence(&pack, &observation);

        assert!(evidence
            .iter()
            .any(|item| item.source == EvidenceSource::Canon));
        assert!(evidence
            .iter()
            .any(|item| item.source == EvidenceSource::PromiseLedger));

        let fallback = context_pack_evidence(&context_pack(Vec::new()), &observation);
        assert_eq!(fallback[0].source, EvidenceSource::ChapterText);
        assert_eq!(fallback[0].reference, "Chapter-1");
    }
}
