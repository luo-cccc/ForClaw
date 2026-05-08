use crate::writer_agent::kernel::{
    chapter_result_from_observation, extract_plot_promises, split_sentences,
};
use crate::writer_agent::memory::{ChapterResultSummary, PlotPromiseSummary, WriterMemory};
use crate::writer_agent::observation::{ObservationReason, ObservationSource, WriterObservation};

pub fn build_basic_chapter_settlement_delta(
    project_id: &str,
    chapter_title: &str,
    chapter_revision: &str,
    generated_content: &str,
    created_at_ms: u64,
    memory: &WriterMemory,
    continuity_issues: Vec<String>,
) -> ChapterSettlementDelta {
    let observation = settlement_observation(
        project_id,
        chapter_title,
        chapter_revision,
        generated_content,
        created_at_ms,
    );
    let chapter_result = chapter_result_from_observation(&observation, memory);
    let open_promises = memory.get_open_promise_summaries().unwrap_or_default();
    let mut extraction = build_settlement_extraction(
        generated_content,
        &observation,
        &chapter_result,
        &open_promises,
        memory,
    );
    // Deduplicate promise candidates by title, keeping highest confidence
    deduplicate_promise_candidates(&mut extraction.promise_candidates);
    // Filter low-confidence candidates and cap total
    extraction
        .promise_candidates
        .retain(|c| c.confidence >= 0.6);
    extraction.promise_candidates.truncate(15);

    let promise_updates =
        materialize_promise_delta_entries(&extraction, &chapter_result, &open_promises);
    let book_state_updates = derive_book_state_updates(&chapter_result, &promise_updates);
    let arc_updates = derive_arc_updates(&chapter_result, &promise_updates);
    let summary = extraction
        .summary_candidates
        .first()
        .map(|candidate| candidate.value.clone())
        .unwrap_or_else(|| chapter_result.summary.clone());

    let character_state_deltas = extraction.character_state_deltas.clone();
    let relationship_deltas = extraction.relationship_deltas.clone();
    let knowledge_deltas = extraction.knowledge_deltas.clone();
    let identity_deltas = extraction.identity_deltas.clone();
    let scene_deltas = extraction.scene_deltas.clone();
    let emotional_debt_cues = extraction.emotional_debt_cues.clone();

    let reader_takeaway = ReaderTakeaway {
        emotional_beat: {
            let change_text = chapter_result.state_changes.join(" ");
            if change_text.contains("冲突") {
                "紧张".to_string()
            } else if change_text.contains("和解") {
                "感动".to_string()
            } else {
                "好奇".to_string()
            }
        },
        expectation: open_promises
            .first()
            .map(|p| p.expected_payoff.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_default(),
        unresolved_lack: {
            let stale: Vec<_> = open_promises
                .iter()
                .filter(|p| !p.last_seen_chapter.is_empty() && p.last_seen_chapter != chapter_title)
                .map(|p| p.title.clone())
                .collect();
            stale.join("、")
        },
    };

    ChapterSettlementDelta {
        chapter_title: chapter_title.to_string(),
        chapter_revision: chapter_revision.to_string(),
        summary,
        extraction,
        chapter_result: ChapterResultDelta {
            summary: chapter_result.summary.clone(),
            state_changes: chapter_result.state_changes.clone(),
            character_progress: chapter_result.character_progress.clone(),
            new_conflicts: chapter_result.new_conflicts.clone(),
            new_clues: chapter_result.new_clues.clone(),
            promise_updates: chapter_result.promise_updates.clone(),
            canon_updates: chapter_result.canon_updates.clone(),
        },
        promise_updates: promise_updates.clone(),
        arc_updates: arc_updates.clone(),
        book_state_updates: book_state_updates.clone(),
        chapter_fact_delta: chapter_fact_lines(&chapter_result),
        promise_delta: promise_updates
            .iter()
            .map(render_promise_delta_line)
            .collect(),
        arc_delta: arc_updates
            .iter()
            .map(|entry| format!("{}: {}", entry.scope, entry.value))
            .collect(),
        book_state_delta: book_state_updates
            .iter()
            .map(render_book_state_delta_line)
            .collect(),
        character_state_deltas,
        relationship_deltas,
        knowledge_deltas,
        identity_deltas,
        scene_deltas,
        continuity_issues,
        repairable: true,
        reader_takeaway: Some(reader_takeaway),
        emotional_debt_cues,
        ..Default::default()
    }
}

fn settlement_observation(
    project_id: &str,
    chapter_title: &str,
    chapter_revision: &str,
    generated_content: &str,
    created_at_ms: u64,
) -> WriterObservation {
    WriterObservation {
        id: format!("settlement-{}-{}", chapter_title, chapter_revision),
        created_at: created_at_ms,
        source: ObservationSource::ChapterSave,
        reason: ObservationReason::Save,
        project_id: project_id.to_string(),
        chapter_title: Some(chapter_title.to_string()),
        chapter_revision: Some(chapter_revision.to_string()),
        cursor: None,
        selection: None,
        prefix: generated_content.to_string(),
        suffix: String::new(),
        paragraph: generated_content
            .lines()
            .rev()
            .find(|line| line.trim().chars().count() >= 8)
            .unwrap_or(generated_content)
            .trim()
            .to_string(),
        full_text_digest: Some(crate::storage::content_revision(generated_content)),
        editor_dirty: false,
    }
}

fn build_settlement_extraction(
    generated_content: &str,
    observation: &WriterObservation,
    chapter_result: &ChapterResultSummary,
    open_promises: &[PlotPromiseSummary],
    memory: &WriterMemory,
) -> ChapterSettlementExtraction {
    let mut promise_candidates = Vec::new();
    let mut chapter_result_candidates = Vec::new();
    let lowercase = generated_content.to_lowercase();

    chapter_result_candidates.push(ChapterResultExtractionCandidate {
        field: "summary".to_string(),
        value: chapter_result.summary.clone(),
        confidence: 0.72,
        evidence: vec![ChapterSettlementEvidence {
            excerpt: chapter_result.summary.clone(),
            signal: "chapter_result_summary".to_string(),
        }],
    });
    for line in &chapter_result.state_changes {
        chapter_result_candidates.push(ChapterResultExtractionCandidate {
            field: "state_change".to_string(),
            value: line.clone(),
            confidence: 0.7,
            evidence: vec![ChapterSettlementEvidence {
                excerpt: line.clone(),
                signal: "state_change_cue".to_string(),
            }],
        });
    }
    for line in &chapter_result.new_conflicts {
        chapter_result_candidates.push(ChapterResultExtractionCandidate {
            field: "new_conflict".to_string(),
            value: line.clone(),
            confidence: 0.68,
            evidence: vec![ChapterSettlementEvidence {
                excerpt: line.clone(),
                signal: "conflict_cue".to_string(),
            }],
        });
    }

    for promise in open_promises {
        let title_hit =
            !promise.title.trim().is_empty() && generated_content.contains(&promise.title);
        let desc_hit = !promise.description.trim().is_empty()
            && split_sentences(&promise.description)
                .into_iter()
                .any(|fragment| generated_content.contains(fragment.trim()));
        if !(title_hit || desc_hit) {
            continue;
        }
        let resolved = title_hit
            && [
                "交代",
                "说出",
                "揭开",
                "归还",
                "放回",
                "找到",
                "解释",
                "兑现",
                "真相大白",
                "水落石出",
            ]
            .iter()
            .any(|cue| lowercase.contains(cue));
        let blocked_reason = if chapter_result
            .new_conflicts
            .iter()
            .any(|line| line.contains(&promise.title))
        {
            chapter_result
                .new_conflicts
                .iter()
                .find(|line| line.contains(&promise.title))
                .cloned()
                .unwrap_or_default()
        } else {
            String::new()
        };
        promise_candidates.push(ChapterPromiseExtractionCandidate {
            action: if resolved {
                "resolved".to_string()
            } else if !blocked_reason.is_empty() {
                "deferred".to_string()
            } else {
                "advanced".to_string()
            },
            kind: promise.kind.clone(),
            title: promise.title.clone(),
            description: promise.description.clone(),
            expected_payoff: if !blocked_reason.is_empty() {
                next_chapter_label(&chapter_result.chapter_title)
            } else {
                promise.expected_payoff.clone()
            },
            confidence: if resolved {
                0.82
            } else if !blocked_reason.is_empty() {
                0.74
            } else {
                0.7
            },
            evidence: vec![
                ChapterSettlementEvidence {
                    excerpt: promise.title.clone(),
                    signal: if title_hit {
                        "title_hit".to_string()
                    } else {
                        "description_hit".to_string()
                    },
                },
                ChapterSettlementEvidence {
                    excerpt: chapter_result.summary.clone(),
                    signal: "result_summary".to_string(),
                },
                ChapterSettlementEvidence {
                    excerpt: blocked_reason.clone(),
                    signal: "blocked_reason".to_string(),
                },
            ]
            .into_iter()
            .filter(|item| !item.excerpt.trim().is_empty())
            .collect(),
        });
    }

    for promise in extract_plot_promises(generated_content, observation) {
        if promise_candidates
            .iter()
            .any(|existing| existing.title == promise.title)
            || open_promises
                .iter()
                .any(|existing| existing.title == promise.title)
        {
            continue;
        }
        promise_candidates.push(ChapterPromiseExtractionCandidate {
            action: "introduced".to_string(),
            kind: promise.kind.clone(),
            title: promise.title.clone(),
            description: promise.description.clone(),
            expected_payoff: promise.expected_payoff.clone(),
            confidence: 0.66,
            evidence: vec![ChapterSettlementEvidence {
                excerpt: promise.description.clone(),
                signal: "new_promise_extracted".to_string(),
            }],
        });
    }

    let summary_candidates = vec![ChapterResultExtractionCandidate {
        field: "summary".to_string(),
        value: chapter_result.summary.clone(),
        confidence: 0.72,
        evidence: vec![ChapterSettlementEvidence {
            excerpt: chapter_result.summary.clone(),
            signal: "chapter_result_summary".to_string(),
        }],
    }];
    let book_state_candidates = chapter_result
        .state_changes
        .iter()
        .filter(|line| looks_irreversible(line))
        .map(|line| ChapterBookStateExtractionCandidate {
            bucket: "irreversible_change".to_string(),
            value: line.clone(),
            confidence: 0.76,
            evidence: vec![ChapterSettlementEvidence {
                excerpt: line.clone(),
                signal: "irreversible_state_change".to_string(),
            }],
        })
        .collect();

    let character_state_deltas: Vec<CharacterStateDeltaEntry> = chapter_result
        .character_progress
        .iter()
        .filter_map(|prog| {
            let parts: Vec<&str> = prog.splitn(2, ':').collect();
            if parts.len() < 2 {
                return None;
            }
            let name = parts[0].trim();
            let detail = parts[1].trim();
            if name.is_empty() || detail.is_empty() {
                return None;
            }
            Some(CharacterStateDeltaEntry {
                character_name: name.to_string(),
                chapter_title: chapter_result.chapter_title.clone(),
                action: "upserted".to_string(),
                core_commitments: vec![detail.to_string()],
                goal_state: serde_json::json!({}),
                source_ref: chapter_result.source_ref.clone(),
            })
        })
        .collect();

    let relationship_deltas: Vec<RelationshipDeltaEntry> = chapter_result
        .new_conflicts
        .iter()
        .filter(|c| {
            let lower = c.to_lowercase();
            lower.contains("关系")
                || lower.contains("盟友")
                || lower.contains("敌对")
                || lower.contains("决裂")
                || lower.contains("结盟")
        })
        .map(|_conflict| RelationshipDeltaEntry {
            character_a_name: String::new(),
            character_b_name: String::new(),
            action: "changed".to_string(),
            relation_type: "complex".to_string(),
            visibility: "public".to_string(),
            chapter_title: chapter_result.chapter_title.clone(),
            source_ref: chapter_result.source_ref.clone(),
        })
        .collect();

    let knowledge_deltas: Vec<KnowledgeDeltaEntry> = {
        let mut deltas = Vec::new();
        let lower_summary = chapter_result.summary.to_lowercase();
        let reveal_cues = [
            "reveal",
            "揭露",
            "揭示",
            "透露",
            "暴露",
            "发现",
            "知道",
            "明白",
            "真相",
            "秘密",
            "真相大白",
            "水落石出",
            "展现",
            "披露",
            "坦白",
            "发现",
        ];
        let has_reveal_cue = reveal_cues.iter().any(|cue| lower_summary.contains(cue))
            || chapter_result.new_conflicts.iter().any(|line| {
                let lower = line.to_lowercase();
                reveal_cues.iter().any(|cue| lower.contains(cue))
            });
        if has_reveal_cue {
            deltas.push(KnowledgeDeltaEntry {
                topic: chapter_result.summary.chars().take(80).collect(),
                truth_state: "revealed".to_string(),
                holder_type: "character".to_string(),
                holder_id: 0,
                knowledge_mode: "known".to_string(),
                chapter_title: chapter_result.chapter_title.clone(),
                source_ref: chapter_result.source_ref.clone(),
            });
        }
        deltas
    };

    let identity_deltas: Vec<IdentityDeltaEntry> = {
        let identity_keywords = [
            "身份",
            "identity",
            "认同",
            "伪装",
            "面具",
            "真面目",
            "真实身份",
            "假身份",
            "扮演",
            "角色",
        ];
        chapter_result
            .character_progress
            .iter()
            .filter(|prog| {
                let lower = prog.to_lowercase();
                identity_keywords.iter().any(|kw| lower.contains(kw))
            })
            .map(|prog| {
                let parts: Vec<&str> = prog.splitn(2, ':').collect();
                let name = parts.first().map(|s| s.trim()).unwrap_or("");
                let detail = parts.get(1).map(|s| s.trim()).unwrap_or("");
                IdentityDeltaEntry {
                    character_name: name.to_string(),
                    public_identity: detail.to_string(),
                    private_identity: String::new(),
                    revealed_to: Vec::new(),
                    chapter_title: chapter_result.chapter_title.clone(),
                    source_ref: chapter_result.source_ref.clone(),
                }
            })
            .collect()
    };

    let scene_deltas: Vec<SceneResultProjection> = {
        let scenes = memory
            .list_scenes_by_chapter(&chapter_result.chapter_title)
            .unwrap_or_default();
        if scenes.is_empty() {
            vec![SceneResultProjection {
                scene_id: 0,
                outcome: chapter_result.summary.clone(),
                consequence: chapter_result
                    .new_conflicts
                    .first()
                    .cloned()
                    .unwrap_or_default(),
                source_ref: chapter_result.source_ref.clone(),
            }]
        } else {
            scenes
                .iter()
                .map(|s| SceneResultProjection {
                    scene_id: s.id,
                    outcome: chapter_result.summary.clone(),
                    consequence: chapter_result
                        .state_changes
                        .first()
                        .cloned()
                        .unwrap_or_default(),
                    source_ref: chapter_result.source_ref.clone(),
                })
                .collect()
        }
    };

    // Emotional debt cue extraction: scan state_changes and new_conflicts for pressure keywords
    let mut emotional_debt_cues = Vec::new();
    let emotional_keywords = [
        "愤怒", "悲伤", "背叛", "恐惧", "失去", "悔恨", "自责", "绝望", "压抑", "痛苦",
    ];
    for line in &chapter_result.state_changes {
        for keyword in &emotional_keywords {
            if line.contains(keyword) && !emotional_debt_cues.contains(&keyword.to_string()) {
                emotional_debt_cues.push(keyword.to_string());
            }
        }
    }
    for line in &chapter_result.new_conflicts {
        for keyword in &emotional_keywords {
            if line.contains(keyword) && !emotional_debt_cues.contains(&keyword.to_string()) {
                emotional_debt_cues.push(keyword.to_string());
            }
        }
    }

    ChapterSettlementExtraction {
        summary_candidates,
        chapter_result_candidates,
        promise_candidates,
        book_state_candidates,
        character_state_deltas,
        relationship_deltas,
        knowledge_deltas,
        identity_deltas,
        scene_deltas,
        warnings: Vec::new(),
        emotional_debt_cues,
    }
}

// Deduplicate promise candidates by title, keeping highest confidence
fn deduplicate_promise_candidates(candidates: &mut Vec<ChapterPromiseExtractionCandidate>) {
    let mut kept: Vec<ChapterPromiseExtractionCandidate> = Vec::new();
    for cand in candidates.drain(..) {
        if let Some(existing) = kept.iter_mut().find(|k| k.title == cand.title) {
            if cand.confidence > existing.confidence {
                *existing = cand;
            }
        } else {
            kept.push(cand);
        }
    }
    *candidates = kept;
}

fn materialize_promise_delta_entries(
    extraction: &ChapterSettlementExtraction,
    chapter_result: &ChapterResultSummary,
    open_promises: &[PlotPromiseSummary],
) -> Vec<ChapterPromiseDeltaEntry> {
    extraction
        .promise_candidates
        .iter()
        .filter(|candidate| candidate.confidence >= 0.6)
        .map(|candidate| {
            let existing = open_promises
                .iter()
                .find(|promise| promise.title == candidate.title && promise.kind == candidate.kind);
            let action = match candidate.action.as_str() {
                "introduced" => ChapterPromiseDeltaAction::Introduced,
                "resolved" => ChapterPromiseDeltaAction::Resolved,
                "deferred" => ChapterPromiseDeltaAction::Deferred,
                _ => ChapterPromiseDeltaAction::Advanced,
            };
            ChapterPromiseDeltaEntry {
                action,
                promise_id: existing.map(|promise| promise.id),
                kind: candidate.kind.clone(),
                title: candidate.title.clone(),
                description: candidate.description.clone(),
                chapter: chapter_result.chapter_title.clone(),
                source_ref: chapter_result.source_ref.clone(),
                expected_payoff: candidate.expected_payoff.clone(),
                priority: existing.map(|promise| promise.priority).unwrap_or(4),
                related_entities: Vec::new(),
                core: existing.map(|promise| promise.core).unwrap_or(false),
                promoted: existing.map(|promise| promise.promoted).unwrap_or(false),
                blocked_reason: if candidate.action == "deferred" {
                    chapter_result
                        .new_conflicts
                        .iter()
                        .find(|line| line.contains(&candidate.title))
                        .cloned()
                        .unwrap_or_default()
                } else {
                    String::new()
                },
                evidence: candidate
                    .evidence
                    .iter()
                    .map(|item| item.excerpt.clone())
                    .collect::<Vec<_>>()
                    .join(" | "),
            }
        })
        .collect()
}

fn derive_book_state_updates(
    chapter_result: &ChapterResultSummary,
    promise_updates: &[ChapterPromiseDeltaEntry],
) -> Vec<ChapterBookStateDeltaEntry> {
    let mut updates = Vec::new();
    for line in chapter_result.state_changes.iter().take(3) {
        if looks_irreversible(line) {
            updates.push(ChapterBookStateDeltaEntry {
                bucket: ChapterBookStateDeltaBucket::IrreversibleChange,
                value: line.clone(),
                source_ref: chapter_result.source_ref.clone(),
                reason: "chapter state change appears durable".to_string(),
            });
        }
    }
    for promise in promise_updates.iter().filter(|entry| entry.core).take(3) {
        updates.push(ChapterBookStateDeltaEntry {
            bucket: ChapterBookStateDeltaBucket::MegaPromise,
            value: format!("{} -> {}", promise.title, promise.expected_payoff),
            source_ref: promise.source_ref.clone(),
            reason: "core promise should remain visible at book scope".to_string(),
        });
    }
    updates
}

fn derive_arc_updates(
    chapter_result: &ChapterResultSummary,
    promise_updates: &[ChapterPromiseDeltaEntry],
) -> Vec<ChapterArcDeltaEntry> {
    let mut updates = Vec::new();
    if let Some(conflict) = chapter_result.new_conflicts.first() {
        updates.push(ChapterArcDeltaEntry {
            scope: "conflict".to_string(),
            value: conflict.clone(),
            reason: "new conflict should shape upcoming arc planning".to_string(),
        });
    }
    if let Some(promoted) = promise_updates.iter().find(|entry| entry.promoted) {
        updates.push(ChapterArcDeltaEntry {
            scope: "hook".to_string(),
            value: promoted.title.clone(),
            reason: "promoted promise should enter arc planning priority".to_string(),
        });
    }
    updates
}

fn chapter_fact_lines(result: &ChapterResultSummary) -> Vec<String> {
    let mut lines = Vec::new();
    if !result.summary.trim().is_empty() {
        lines.push(result.summary.clone());
    }
    lines.extend(result.state_changes.iter().take(3).cloned());
    lines.extend(result.character_progress.iter().take(2).cloned());
    lines.extend(
        result
            .new_clues
            .iter()
            .map(|clue| format!("线索: {}", clue))
            .take(2),
    );
    lines
}

fn render_promise_delta_line(entry: &ChapterPromiseDeltaEntry) -> String {
    let action = match entry.action {
        ChapterPromiseDeltaAction::Introduced => "introduced",
        ChapterPromiseDeltaAction::Advanced => "advanced",
        ChapterPromiseDeltaAction::Resolved => "resolved",
        ChapterPromiseDeltaAction::Deferred => "deferred",
        ChapterPromiseDeltaAction::Abandoned => "abandoned",
    };
    let mut line = format!("{}: {}", action, entry.title);
    if !entry.expected_payoff.trim().is_empty() {
        line.push_str(&format!(" -> {}", entry.expected_payoff));
    }
    if !entry.blocked_reason.trim().is_empty() {
        line.push_str(&format!(" | blocked: {}", entry.blocked_reason));
    }
    line
}

fn render_book_state_delta_line(entry: &ChapterBookStateDeltaEntry) -> String {
    let bucket = match entry.bucket {
        ChapterBookStateDeltaBucket::LongTermConstraint => "constraint",
        ChapterBookStateDeltaBucket::MegaPromise => "mega_promise",
        ChapterBookStateDeltaBucket::IrreversibleChange => "irreversible_change",
    };
    format!("{}: {}", bucket, entry.value)
}

fn looks_irreversible(line: &str) -> bool {
    [
        "失去", "死亡", "断绝", "背叛", "毁掉", "归还", "公开", "暴露",
    ]
    .iter()
    .any(|cue| line.contains(cue))
}

fn next_chapter_label(chapter: &str) -> String {
    let digits = chapter
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits
        .parse::<i64>()
        .ok()
        .map(|number| format!("Chapter-{}", number + 1))
        .unwrap_or_else(|| "later chapter".to_string())
}

fn hash_str(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn replay_settlement_extraction(
    original: &ChapterSettlementDelta,
    generated_content: &str,
    memory: &WriterMemory,
) -> SettlementReplayResult {
    let replayed = build_basic_chapter_settlement_delta(
        &String::new(),
        &original.chapter_title,
        &original.chapter_revision,
        generated_content,
        0,
        memory,
        original.continuity_issues.clone(),
    );

    let mut mismatches = Vec::new();

    if original.summary != replayed.summary {
        mismatches.push("summary differs".to_string());
    }

    if original.promise_updates.len() != replayed.promise_updates.len() {
        mismatches.push(format!(
            "promise_updates count differs: {} vs {}",
            original.promise_updates.len(),
            replayed.promise_updates.len()
        ));
    }

    if original.chapter_fact_delta.len() != replayed.chapter_fact_delta.len() {
        mismatches.push(format!(
            "chapter_fact_delta count differs: {} vs {}",
            original.chapter_fact_delta.len(),
            replayed.chapter_fact_delta.len()
        ));
    }

    if original.book_state_updates.len() != replayed.book_state_updates.len() {
        mismatches.push(format!(
            "book_state_updates count differs: {} vs {}",
            original.book_state_updates.len(),
            replayed.book_state_updates.len()
        ));
    }

    let original_json = serde_json::to_string(original).unwrap_or_default();
    let replayed_json = serde_json::to_string(&replayed).unwrap_or_default();

    SettlementReplayResult {
        replayed: true,
        matches_original: mismatches.is_empty(),
        mismatches,
        original_hash: hash_str(&original_json),
        replayed_hash: hash_str(&replayed_json),
    }
}
