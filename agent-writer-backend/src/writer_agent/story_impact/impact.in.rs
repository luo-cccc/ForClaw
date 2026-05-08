pub fn compute_story_impact_radius(
    seeds: &[WriterStoryGraphNode],
    graph_nodes: &[WriterStoryGraphNode],
    graph_edges: &[WriterStoryGraphEdge],
    budget_limit: usize,
) -> WriterStoryImpactRadius {
    let mut impacted_ids = std::collections::HashSet::new();
    let mut dropped_ids = std::collections::HashSet::new();
    let mut dropped_nodes = Vec::new();
    let mut impacted_edges = Vec::new();
    let mut reasons = Vec::new();
    let mut total_chars: usize = 0;
    let mut truncated = false;

    for seed in seeds {
        impacted_ids.insert(seed.id.clone());
        total_chars += seed.summary.chars().count();
        reasons.push(format!("种子节点: {}", seed.label));
    }

    // Build an undirected adjacency index so radius catches both dependencies
    // and dependents, matching the conservative blast-radius discipline.
    let mut adjacency: std::collections::HashMap<&str, Vec<&WriterStoryGraphEdge>> =
        std::collections::HashMap::new();
    for edge in graph_edges {
        adjacency.entry(edge.from.as_str()).or_default().push(edge);
        adjacency.entry(edge.to.as_str()).or_default().push(edge);
    }
    // Build node lookup: node_id → &node.
    let node_lookup: std::collections::HashMap<&str, &WriterStoryGraphNode> =
        graph_nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // BFS traversal from seeds with adjacency-indexed edges.
    let mut frontier: Vec<String> = seeds.iter().map(|s| s.id.clone()).collect();
    let mut depth = 0;

    while depth < MAX_TRAVERSAL_DEPTH && !frontier.is_empty() {
        let mut next_frontier = Vec::new();
        let mut candidates: Vec<(&WriterStoryGraphNode, &WriterStoryGraphEdge, &str)> = Vec::new();

        for node_id in &frontier {
            let adjacent = adjacency.get(node_id.as_str());
            for edge in adjacent.into_iter().flatten() {
                let target_id = if edge.from == *node_id {
                    edge.to.as_str()
                } else {
                    edge.from.as_str()
                };
                if impacted_ids.contains(target_id) || dropped_ids.contains(target_id) {
                    continue;
                }
                if let Some(target) = node_lookup.get(target_id) {
                    candidates.push((target, edge, node_id.as_str()));
                }
            }
        }

        candidates.sort_by(|(left, left_edge, _), (right, right_edge, _)| {
            story_impact_candidate_score(right, right_edge)
                .cmp(&story_impact_candidate_score(left, left_edge))
                .then_with(|| {
                    left.summary
                        .chars()
                        .count()
                        .cmp(&right.summary.chars().count())
                })
                .then_with(|| left.id.cmp(&right.id))
        });

        for (target, edge, from_id) in candidates {
            if impacted_ids.contains(target.id.as_str()) || dropped_ids.contains(target.id.as_str())
            {
                continue;
            }
            let node_chars = target.summary.chars().count();
            if total_chars + node_chars > budget_limit {
                truncated = true;
                if dropped_ids.insert(target.id.clone()) {
                    dropped_nodes.push((*target).clone());
                }
                if should_report_budget_drop(target) {
                    reasons.push(format!(
                        "预算截断: {} (需要 {} 字符, 剩余 {} 字符)",
                        target.label,
                        node_chars,
                        budget_limit.saturating_sub(total_chars)
                    ));
                }
                continue;
            }
            total_chars += node_chars;
            impacted_ids.insert(target.id.clone());
            next_frontier.push(target.id.clone());
            impacted_edges.push((*edge).clone());
            reasons.push(format!(
                "距离 {}: {} -> {} ({})",
                depth + 1,
                from_id,
                target.label,
                edge.kind.as_str()
            ));
        }

        frontier = next_frontier;
        depth += 1;
    }

    if !frontier.is_empty() {
        truncated = true;
        reasons.push(format!(
            "遍历深度限制 ({}) {} 个节点未展开",
            MAX_TRAVERSAL_DEPTH,
            frontier.len()
        ));
    }

    let impacted_nodes: Vec<WriterStoryGraphNode> = graph_nodes
        .iter()
        .filter(|n| impacted_ids.contains(&n.id))
        .cloned()
        .collect();

    let impacted_sources: Vec<String> = impacted_nodes
        .iter()
        .map(|n| n.source_ref.clone())
        .collect();

    let has_high_risk = impacted_nodes.iter().any(|n| n.confidence < 0.6)
        || impacted_nodes
            .iter()
            .any(|n| matches!(n.kind, StoryNodeKind::PlotPromise));
    let has_medium_risk = truncated
        || impacted_nodes
            .iter()
            .any(|n| matches!(n.kind, StoryNodeKind::CanonEntity));
    let risk = if has_high_risk {
        StoryImpactRisk::High
    } else if has_medium_risk {
        StoryImpactRisk::Medium
    } else {
        StoryImpactRisk::Low
    };

    WriterStoryImpactRadius {
        seed_nodes: seeds.to_vec(),
        impacted_nodes,
        impacted_sources,
        dropped_nodes,
        edges: impacted_edges,
        risk,
        truncated,
        reasons,
    }
}

fn story_impact_candidate_score(node: &WriterStoryGraphNode, edge: &WriterStoryGraphEdge) -> i32 {
    story_node_priority(&node.kind)
        + story_edge_priority(&edge.kind)
        + (node.confidence * 10.0).round() as i32
}

fn story_node_priority(kind: &StoryNodeKind) -> i32 {
    match kind {
        StoryNodeKind::PlotPromise => 90,
        StoryNodeKind::ChapterMission => 80,
        StoryNodeKind::StoryContract => 75,
        StoryNodeKind::CanonRule => 70,
        StoryNodeKind::CanonEntity => 65,
        StoryNodeKind::ResultFeedback => 55,
        StoryNodeKind::Decision => 45,
        StoryNodeKind::ProjectBrainChunk => 35,
        StoryNodeKind::SeedTask => 100,
    }
}

fn story_edge_priority(kind: &StoryEdgeKind) -> i32 {
    match kind {
        StoryEdgeKind::UpdatesPromise => 35,
        StoryEdgeKind::SupportsMission => 30,
        StoryEdgeKind::ContradictsCanon => 30,
        StoryEdgeKind::DependsOnResult => 20,
        StoryEdgeKind::MentionsEntity => 15,
        StoryEdgeKind::SameSourceRevision => 8,
        StoryEdgeKind::SharedKeyword => 5,
    }
}

fn should_report_budget_drop(node: &WriterStoryGraphNode) -> bool {
    node.confidence > 0.7
        || matches!(
            node.kind,
            StoryNodeKind::PlotPromise
                | StoryNodeKind::ChapterMission
                | StoryNodeKind::StoryContract
                | StoryNodeKind::CanonRule
                | StoryNodeKind::CanonEntity
        )
}

/// Compute impact radius and produce a budget report.
pub fn compute_story_impact(
    observation: &WriterObservation,
    context_pack: &WritingContextPack,
    memory: &WriterMemory,
    budget_limit: Option<usize>,
) -> (WriterStoryImpactRadius, StoryImpactBudgetReport) {
    let budget = budget_limit.unwrap_or(DEFAULT_IMPACT_BUDGET_CHARS);
    let seeds = extract_seed_nodes(observation, context_pack, memory);
    let (graph_nodes, graph_edges) = build_story_graph(memory, &observation.project_id);

    let radius = compute_story_impact_radius(&seeds, &graph_nodes, &graph_edges, budget);

    let provided_chars: usize = radius
        .impacted_nodes
        .iter()
        .map(|n| n.summary.chars().count())
        .sum();
    let requested_chars: usize = graph_nodes.iter().map(|n| n.summary.chars().count()).sum();

    let dropped_high_risk: Vec<String> = radius
        .dropped_nodes
        .iter()
        .filter(|node| should_report_budget_drop(node))
        .map(|node| format!("{}: {}", node.source_ref, compact_chars(&node.label, 80)))
        .collect();

    let report = StoryImpactBudgetReport {
        budget_limit: budget,
        requested_chars,
        provided_chars,
        truncated_node_count: radius.dropped_nodes.len(),
        dropped_high_risk_sources: dropped_high_risk,
        reasons: radius.reasons.clone(),
    };

    (radius, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(
        id: &str,
        kind: StoryNodeKind,
        label: &str,
        confidence: f32,
        summary: &str,
    ) -> WriterStoryGraphNode {
        WriterStoryGraphNode {
            id: id.to_string(),
            kind,
            label: label.to_string(),
            source_ref: format!("src:{}", id),
            source_revision: None,
            chapter: None,
            confidence,
            summary: summary.to_string(),
        }
    }

    fn make_edge(from: &str, to: &str, kind: StoryEdgeKind) -> WriterStoryGraphEdge {
        WriterStoryGraphEdge {
            from: from.to_string(),
            to: to.to_string(),
            kind,
            evidence_ref: format!("edge:{}->{}", from, to),
            confidence: 0.8,
        }
    }

    #[test]
    fn includes_impacted_promise_under_budget() {
        let seeds = vec![make_node(
            "task:1",
            StoryNodeKind::SeedTask,
            "当前任务",
            1.0,
            "写作第三章",
        )];
        let nodes = vec![
            seeds[0].clone(),
            make_node(
                "promise:1",
                StoryNodeKind::PlotPromise,
                "寒玉戒指去向",
                0.85,
                "伏笔: 寒玉戒指的去向",
            ),
            make_node(
                "canon:1",
                StoryNodeKind::CanonEntity,
                "林墨",
                0.9,
                "角色: 林墨",
            ),
        ];
        let edges = vec![
            make_edge("task:1", "promise:1", StoryEdgeKind::UpdatesPromise),
            make_edge("task:1", "canon:1", StoryEdgeKind::MentionsEntity),
        ];

        let radius = compute_story_impact_radius(&seeds, &nodes, &edges, 500);

        assert!(
            radius
                .impacted_nodes
                .iter()
                .any(|n| n.kind == StoryNodeKind::PlotPromise),
            "Promise should be in impact radius"
        );
        assert!(!radius.truncated);
    }

    #[test]
    fn excludes_semantic_distractor() {
        let seeds = vec![make_node(
            "task:1",
            StoryNodeKind::SeedTask,
            "当前任务",
            1.0,
            "写作第三章：寒玉戒指",
        )];
        let nodes = vec![
            seeds[0].clone(),
            make_node(
                "canon:1",
                StoryNodeKind::CanonEntity,
                "霜铃塔",
                0.9,
                "地点: 霜铃塔",
            ),
            make_node(
                "canon:2",
                StoryNodeKind::CanonEntity,
                "旧门传闻",
                0.3,
                "无关: old_gate_rumor",
            ),
        ];
        let edges = vec![make_edge(
            "task:1",
            "canon:1",
            StoryEdgeKind::MentionsEntity,
        )];
        // canon:2 has NO edge — should be excluded.

        let radius = compute_story_impact_radius(&seeds, &nodes, &edges, 500);

        assert!(
            radius.impacted_nodes.iter().any(|n| n.id == "canon:1"),
            "Connected canon should be included"
        );
        assert!(
            !radius.impacted_nodes.iter().any(|n| n.id == "canon:2"),
            "Disconnected distractor should be excluded"
        );
    }

    #[test]
    fn reports_truncated_sources() {
        let seeds = vec![make_node(
            "task:1",
            StoryNodeKind::SeedTask,
            "当前任务",
            1.0,
            "写作",
        )];
        let mut nodes = vec![seeds[0].clone()];
        let mut edges = Vec::new();
        for i in 1..=20 {
            nodes.push(make_node(
                &format!("promise:{}", i),
                StoryNodeKind::PlotPromise,
                &format!("伏笔{}", i),
                0.8,
                "这是一个需要大量字符的总结文本用来模拟长内容数据",
            ));
            edges.push(make_edge(
                "task:1",
                &format!("promise:{}", i),
                StoryEdgeKind::UpdatesPromise,
            ));
        }

        let radius = compute_story_impact_radius(&seeds, &nodes, &edges, 200);

        assert!(radius.truncated, "Should be truncated under tight budget");
        assert!(
            radius
                .reasons
                .iter()
                .any(|r| r.contains("预算截断") || r.contains("深度")),
            "Should report truncation reason: {:?}",
            radius.reasons
        );
        assert!(
            radius.impacted_nodes.len() < nodes.len(),
            "Should have fewer impacted nodes than total ({} < {})",
            radius.impacted_nodes.len(),
            nodes.len()
        );
    }

    #[test]
    fn maps_operation_to_story_nodes() {
        let seeds = vec![
            make_node(
                "task:1",
                StoryNodeKind::SeedTask,
                "inline rewrite",
                1.0,
                "重写第三段",
            ),
            make_node(
                "mission:ch3",
                StoryNodeKind::ChapterMission,
                "第三章任务",
                0.9,
                "推进寒玉戒指线索",
            ),
        ];
        let nodes = vec![
            seeds[0].clone(),
            seeds[1].clone(),
            make_node(
                "promise:1",
                StoryNodeKind::PlotPromise,
                "寒玉戒指去向",
                0.85,
                "伏笔: 寒玉戒指去向",
            ),
        ];
        let edges = vec![make_edge(
            "mission:ch3",
            "promise:1",
            StoryEdgeKind::SupportsMission,
        )];

        let radius = compute_story_impact_radius(&seeds, &nodes, &edges, 500);

        assert!(
            radius.impacted_nodes.len() >= 2,
            "Should include mission + promise"
        );
        assert!(
            radius
                .impacted_sources
                .iter()
                .any(|s| s.contains("promise:1")),
            "Promise should be in impacted sources"
        );
    }

    #[test]
    fn traverses_reverse_edges_for_dependents() {
        let seeds = vec![make_node(
            "canon:1",
            StoryNodeKind::CanonEntity,
            "寒玉戒指",
            0.9,
            "实体: 寒玉戒指",
        )];
        let nodes = vec![
            seeds[0].clone(),
            make_node(
                "mission:ch3",
                StoryNodeKind::ChapterMission,
                "第三章任务",
                0.9,
                "推进寒玉戒指线索",
            ),
        ];
        let edges = vec![make_edge(
            "mission:ch3",
            "canon:1",
            StoryEdgeKind::SupportsMission,
        )];

        let radius = compute_story_impact_radius(&seeds, &nodes, &edges, 500);

        assert!(
            radius.impacted_nodes.iter().any(|n| n.id == "mission:ch3"),
            "Reverse traversal should include mission depending on seeded canon"
        );
    }

    #[test]
    fn small_change_stays_minimal() {
        let seeds = vec![make_node(
            "task:1",
            StoryNodeKind::SeedTask,
            "光标微调",
            1.0,
            "fix typo",
        )];
        let nodes = vec![
            seeds[0].clone(),
            make_node("canon:1", StoryNodeKind::CanonEntity, "林墨", 0.9, "角色"),
        ];
        let edges = vec![];

        let radius = compute_story_impact_radius(&seeds, &nodes, &edges, 500);

        assert_eq!(radius.impacted_nodes.len(), 1);
        assert!(!radius.truncated);
        assert!(matches!(radius.risk, StoryImpactRisk::Low));
    }
}
