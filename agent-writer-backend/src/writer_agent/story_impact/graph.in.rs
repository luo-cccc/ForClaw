pub fn story_impact_task_summary(
    radius: &WriterStoryImpactRadius,
    budget: &StoryImpactBudgetReport,
) -> String {
    let key_nodes = radius
        .impacted_nodes
        .iter()
        .filter(|node| !matches!(node.kind, StoryNodeKind::SeedTask))
        .take(6)
        .map(|node| format!("{}:{}", node.kind.as_str(), compact_chars(&node.label, 48)))
        .collect::<Vec<_>>()
        .join(", ");
    let reasons = radius
        .reasons
        .iter()
        .take(4)
        .map(|reason| compact_chars(reason, 96))
        .collect::<Vec<_>>()
        .join(" | ");
    compact_chars(
        &format!(
            "risk={:?}; impactedNodes={}; edges={}; budget={}/{} chars; truncated={}; keyNodes={}; reasons={}",
            radius.risk,
            radius.impacted_nodes.len(),
            radius.edges.len(),
            budget.provided_chars,
            budget.budget_limit,
            radius.truncated,
            if key_nodes.is_empty() { "none" } else { &key_nodes },
            if reasons.is_empty() { "none" } else { &reasons },
        ),
        480,
    )
}

pub fn story_impact_context_summary(
    radius: &WriterStoryImpactRadius,
    budget: &StoryImpactBudgetReport,
) -> String {
    let key_nodes = radius
        .impacted_nodes
        .iter()
        .filter(|node| !matches!(node.kind, StoryNodeKind::SeedTask))
        .take(8)
        .map(|node| {
            format!(
                "- {} [{} confidence {:.2}]: {}",
                compact_chars(&node.label, 64),
                node.kind.as_str(),
                node.confidence,
                compact_chars(&node.summary, 140)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let reasons = radius
        .reasons
        .iter()
        .take(6)
        .map(|reason| format!("- {}", compact_chars(reason, 140)))
        .collect::<Vec<_>>()
        .join("\n");
    let dropped = if budget.dropped_high_risk_sources.is_empty() {
        "none".to_string()
    } else {
        budget
            .dropped_high_risk_sources
            .iter()
            .take(4)
            .map(|source| compact_chars(source, 120))
            .collect::<Vec<_>>()
            .join("; ")
    };

    format!(
        "Story Impact Radius\nrisk: {:?}\nimpacted nodes: {}\nedges: {}\nbudget: {}/{} chars\ntruncated: {}\ndropped high-risk sources: {}\nkey impacted nodes:\n{}\nwhy included:\n{}",
        radius.risk,
        radius.impacted_nodes.len(),
        radius.edges.len(),
        budget.provided_chars,
        budget.budget_limit,
        radius.truncated,
        dropped,
        if key_nodes.trim().is_empty() {
            "- none".to_string()
        } else {
            key_nodes
        },
        if reasons.trim().is_empty() {
            "- none".to_string()
        } else {
            reasons
        },
    )
}

fn compact_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut out = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// Extract seed nodes from the current observation and context pack.
pub fn extract_seed_nodes(
    observation: &WriterObservation,
    context_pack: &WritingContextPack,
    memory: &WriterMemory,
) -> Vec<WriterStoryGraphNode> {
    let mut seeds = Vec::new();

    seeds.push(WriterStoryGraphNode {
        id: format!("task:{}", observation.id),
        kind: StoryNodeKind::SeedTask,
        label: "当前写作任务".to_string(),
        source_ref: format!("observation:{}", observation.id),
        source_revision: observation.chapter_revision.clone(),
        chapter: observation.chapter_title.clone(),
        confidence: 1.0,
        summary: format!("{:?} observation", observation.source),
    });

    // Current chapter mission.
    if let Some(chapter) = &observation.chapter_title {
        if let Ok(Some(mission)) = memory.get_chapter_mission(&observation.project_id, chapter) {
            seeds.push(WriterStoryGraphNode {
                id: format!("mission:{}", mission.chapter_title),
                kind: StoryNodeKind::ChapterMission,
                label: format!("章节任务: {}", mission.chapter_title),
                source_ref: format!("chapter_mission:{}", mission.chapter_title),
                source_revision: None,
                chapter: Some(mission.chapter_title.clone()),
                confidence: 0.9,
                summary: mission.mission,
            });
        }
    }

    // Open promises (top 3 by priority).
    if let Ok(open) = memory.get_open_promise_summaries() {
        for promise in open.iter().take(3) {
            seeds.push(WriterStoryGraphNode {
                id: format!("promise:{}", promise.id),
                kind: StoryNodeKind::PlotPromise,
                label: promise.title.clone(),
                source_ref: format!("promise:{}", promise.id),
                source_revision: None,
                chapter: Some(promise.introduced_chapter.clone()),
                confidence: 0.85,
                summary: promise.description.clone(),
            });
        }
    }

    // Canon entities mentioned in cursor / selected text.
    if let Ok(entity_names) = memory.get_canon_entity_names() {
        for source in &context_pack.sources {
            if matches!(
                source.source,
                ContextSource::CursorPrefix | ContextSource::SelectedText
            ) {
                let text_lower = source.content.to_lowercase();
                for name in entity_names.iter().take(5) {
                    if text_lower.contains(&name.to_lowercase()) {
                        seeds.push(WriterStoryGraphNode {
                            id: format!("canon:{}", name),
                            kind: StoryNodeKind::CanonEntity,
                            label: name.clone(),
                            source_ref: format!("canon:{}", name),
                            source_revision: None,
                            chapter: None,
                            confidence: 0.8,
                            summary: format!("实体: {}", name),
                        });
                    }
                }
                break;
            }
        }
    }

    seeds
}

/// Build an in-memory story graph from memory ledger data.
pub fn build_story_graph(
    memory: &WriterMemory,
    project_id: &str,
) -> (Vec<WriterStoryGraphNode>, Vec<WriterStoryGraphEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Canon entities.
    if let Ok(entities) = memory.list_canon_entities() {
        for entity in &entities {
            let node_id = format!("canon:{}", entity.name);
            let attrs_str = entity
                .attributes
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .map(|(k, v)| format!("{}: {}", k, v))
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();

            nodes.push(WriterStoryGraphNode {
                id: node_id.clone(),
                kind: StoryNodeKind::CanonEntity,
                label: entity.name.clone(),
                source_ref: format!("canon:{}", entity.name),
                source_revision: None,
                chapter: None,
                confidence: entity.confidence as f32,
                summary: if attrs_str.is_empty() {
                    entity.summary.clone()
                } else {
                    attrs_str
                },
            });
        }
    }

    // Canon rules.
    if let Ok(rules) = memory.list_canon_rules(20) {
        for rule in &rules {
            let node_id = format!("canon_rule:{}", rule.rule);
            nodes.push(WriterStoryGraphNode {
                id: node_id,
                kind: StoryNodeKind::CanonRule,
                label: rule.rule.clone(),
                source_ref: format!("canon_rule:{}", rule.rule),
                source_revision: None,
                chapter: None,
                confidence: rule.priority as f32 / 10.0,
                summary: format!("类别: {}, 状态: {}", rule.category, rule.status),
            });
        }
    }

    // Open promises (with full summaries).
    if let Ok(promises) = memory.get_open_promise_summaries() {
        for p in &promises {
            let node_id = format!("promise:{}", p.id);
            nodes.push(WriterStoryGraphNode {
                id: node_id.clone(),
                kind: StoryNodeKind::PlotPromise,
                label: p.title.clone(),
                source_ref: format!("promise:{}", p.id),
                source_revision: None,
                chapter: Some(p.introduced_chapter.clone()),
                confidence: if p.risk == "high" { 0.5 } else { 0.85 },
                summary: format!(
                    "kind={} payoff={} risk={} desc={}",
                    p.kind, p.expected_payoff, p.risk, p.description
                ),
            });

            // Edge: promise ↔ entity by name overlap.
            for other in &nodes {
                if other.id != node_id {
                    let label_lower = p.title.to_lowercase();
                    if label_lower.contains(&other.label.to_lowercase())
                        || other.label.to_lowercase().contains(&label_lower)
                    {
                        edges.push(WriterStoryGraphEdge {
                            from: node_id.clone(),
                            to: other.id.clone(),
                            kind: StoryEdgeKind::UpdatesPromise,
                            evidence_ref: format!("promise_entity_overlap:{}:{}", p.id, other.id),
                            confidence: 0.6,
                        });
                    }
                }
            }
        }
    }

    // Chapter missions.
    if let Ok(missions) = memory.list_chapter_missions(project_id, 20) {
        for m in &missions {
            let node_id = format!("mission:{}", m.chapter_title);
            nodes.push(WriterStoryGraphNode {
                id: node_id.clone(),
                kind: StoryNodeKind::ChapterMission,
                label: format!("章节任务: {}", m.chapter_title),
                source_ref: format!("chapter_mission:{}", m.chapter_title),
                source_revision: None,
                chapter: Some(m.chapter_title.clone()),
                confidence: 0.9,
                summary: m.mission.clone(),
            });

            // Mission → entity edges.
            let mission_text = format!(
                "{} {} {} {}",
                m.mission, m.must_include, m.must_not, m.expected_ending
            )
            .to_lowercase();
            for other in &nodes {
                if other.id != node_id && mission_text.contains(&other.label.to_lowercase()) {
                    edges.push(WriterStoryGraphEdge {
                        from: node_id.clone(),
                        to: other.id.clone(),
                        kind: StoryEdgeKind::SupportsMission,
                        evidence_ref: format!("mission_mentions:{}:{}", m.chapter_title, other.id),
                        confidence: 0.75,
                    });
                }
            }
        }
    }

    // Recent chapter result feedback.
    if let Ok(results) = memory.list_recent_chapter_results(project_id, 5) {
        for r in &results {
            let node_id = format!("feedback:{}", r.id);
            nodes.push(WriterStoryGraphNode {
                id: node_id.clone(),
                kind: StoryNodeKind::ResultFeedback,
                label: format!("章节反馈: {}", r.chapter_title),
                source_ref: format!("result_feedback:{}", r.id),
                source_revision: Some(r.chapter_revision.clone()),
                chapter: Some(r.chapter_title.clone()),
                confidence: 0.8,
                summary: r.summary.clone(),
            });

            // Feedback → entity edges via promise_updates / canon_updates.
            for update in r.promise_updates.iter().chain(r.canon_updates.iter()) {
                for other in &nodes {
                    if other.id != node_id
                        && update.to_lowercase().contains(&other.label.to_lowercase())
                    {
                        edges.push(WriterStoryGraphEdge {
                            from: node_id.clone(),
                            to: other.id.clone(),
                            kind: StoryEdgeKind::DependsOnResult,
                            evidence_ref: format!("feedback_update:{}:{}", r.id, other.id),
                            confidence: 0.7,
                        });
                    }
                }
            }
        }
    }

    // Story contract.
    if let Ok(Some(contract)) = memory.get_story_contract(project_id) {
        nodes.push(WriterStoryGraphNode {
            id: "story_contract".to_string(),
            kind: StoryNodeKind::StoryContract,
            label: "故事合同".to_string(),
            source_ref: "story_contract".to_string(),
            source_revision: None,
            chapter: None,
            confidence: 1.0,
            summary: format!("{}: {}", contract.title, contract.reader_promise),
        });
    }

    // Recent creative decisions.
    if let Ok(decisions) = memory.list_recent_decisions(10) {
        for d in &decisions {
            let node_id = format!("decision:{}:{}", d.scope, d.title);
            nodes.push(WriterStoryGraphNode {
                id: node_id,
                kind: StoryNodeKind::Decision,
                label: d.title.clone(),
                source_ref: format!("decision:{}:{}", d.scope, d.title),
                source_revision: None,
                chapter: Some(d.scope.clone()),
                confidence: 0.85,
                summary: d.rationale.clone(),
            });
        }
    }

    // ContradictsCanon edges: canon rule ↔ entity where the rule restricts the entity.
    let canon_entity_ids: Vec<String> = nodes
        .iter()
        .filter(|n| matches!(n.kind, StoryNodeKind::CanonEntity))
        .map(|n| n.id.clone())
        .collect();
    let canon_rule_ids: Vec<String> = nodes
        .iter()
        .filter(|n| matches!(n.kind, StoryNodeKind::CanonRule))
        .map(|n| n.id.clone())
        .collect();
    for rule_id in &canon_rule_ids {
        if let Some(rule_node) = nodes.iter().find(|n| &n.id == rule_id) {
            let rule_lower = rule_node.label.to_lowercase();
            for entity_id in &canon_entity_ids {
                if let Some(entity_node) = nodes.iter().find(|n| &n.id == entity_id) {
                    let entity_label_lower = entity_node.label.to_lowercase();
                    // Edge if rule text mentions the entity, or entity summary mentions rule category.
                    if rule_lower.contains(&entity_label_lower)
                        || entity_node.summary.to_lowercase().contains(&rule_lower)
                    {
                        edges.push(WriterStoryGraphEdge {
                            from: rule_id.clone(),
                            to: entity_id.clone(),
                            kind: StoryEdgeKind::ContradictsCanon,
                            evidence_ref: format!(
                                "canon_rule_entity:{}:{}",
                                rule_node.label, entity_node.label
                            ),
                            confidence: 0.65,
                        });
                    }
                }
            }
        }
    }

    // SameSourceRevision edges: nodes sharing the same chapter or source revision.
    for i in 0..nodes.len() {
        for j in (i + 1)..nodes.len() {
            let a = &nodes[i];
            let b = &nodes[j];
            let share_chapter =
                a.chapter.is_some() && b.chapter.is_some() && a.chapter == b.chapter;
            let share_revision = a.source_revision.is_some()
                && b.source_revision.is_some()
                && a.source_revision == b.source_revision;
            if share_chapter || share_revision {
                edges.push(WriterStoryGraphEdge {
                    from: a.id.clone(),
                    to: b.id.clone(),
                    kind: StoryEdgeKind::SameSourceRevision,
                    evidence_ref: format!(
                        "same_source:{}:{}",
                        a.chapter.as_deref().unwrap_or("?"),
                        b.chapter.as_deref().unwrap_or("?")
                    ),
                    confidence: 0.5,
                });
            }
        }
    }

    (nodes, edges)
}

