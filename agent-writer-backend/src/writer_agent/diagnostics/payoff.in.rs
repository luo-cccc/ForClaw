impl DiagnosticsEngine {
    pub fn diagnose_payoff(
        &self,
        chapter_id: &str,
        project_id: &str,
        memory: &WriterMemory,
    ) -> Vec<DiagnosticResult> {
        let mut results = Vec::new();
        let mut counter = 0u32;
        let mut next_id = || {
            counter += 1;
            format!("payoff_{}_{}", chapter_id, counter)
        };

        let open_debts = memory
            .get_open_emotional_debts(project_id)
            .unwrap_or_default();
        let chapter_mission = memory
            .get_chapter_mission(project_id, chapter_id)
            .unwrap_or_default();

        // 1. Pressure without payoff: open debts past their expected window
        for debt in &open_debts {
            let overdue = debt.overdue_risk == "high"
                || (!debt.expected_payoff_window.is_empty()
                    && debt.expected_payoff_window.contains(chapter_id));
            if overdue {
                results.push(DiagnosticResult {
                    id: next_id(),
                    severity: DiagnosticSeverity::Warning,
                    category: DiagnosticCategory::PayoffGap,
                    message: format!(
                        "情绪债务 '{}' ({}): 预期兑现窗口 '{}', 状态仍为 '{}'",
                        debt.title,
                        debt.debt_kind,
                        debt.expected_payoff_window,
                        debt.payoff_status,
                    ),
                    entity_name: Some(debt.title.clone()),
                    from: 0,
                    to: 0,
                    evidence: vec![],
                    fix_suggestion: Some(format!(
                        "本章考虑回收 '{}' 的情绪债务，或更新预期兑现窗口",
                        debt.title
                    )),
                    operations: vec![],
                });
            }
        }

        // 2. Payoff without pressure: chapter mission has payoff_target but no pressure_scene
        if let Some(ref mission) = chapter_mission {
            if !mission.payoff_target.trim().is_empty()
                && mission.pressure_scene.trim().is_empty()
            {
                results.push(DiagnosticResult {
                    id: next_id(),
                    severity: DiagnosticSeverity::Warning,
                    category: DiagnosticCategory::PayoffGap,
                    message: "本章设置了补偿目标，但缺少前置压迫场景。读者情绪收获需要有对应的情绪投入。"
                        .to_string(),
                    entity_name: Some(chapter_id.to_string()),
                    from: 0,
                    to: 0,
                    evidence: vec![],
                    fix_suggestion: Some("在前文增加压迫场景，使本章补偿更有力".to_string()),
                    operations: vec![],
                });
            }
        }

        // 3. Overfilled lack: all open debts resolved, no next_lack_opened
        if let Some(ref mission) = chapter_mission {
            if mission.next_lack_opened.trim().is_empty()
                && !mission.payoff_target.trim().is_empty()
                && open_debts.len() <= 1
            {
                results.push(DiagnosticResult {
                    id: next_id(),
                    severity: DiagnosticSeverity::Info,
                    category: DiagnosticCategory::PayoffGap,
                    message: "本章补偿点回收后，未打开新的读者缺口。追读可能在此处断裂。".to_string(),
                    entity_name: Some(chapter_id.to_string()),
                    from: 0,
                    to: 0,
                    evidence: vec![],
                    fix_suggestion: Some(
                        "在章节结尾设置新的悬念、冲突或未解问题".to_string(),
                    ),
                    operations: vec![],
                });
            }
        }

        // 4. Relationship soil gap: debts without anchoring relationship
        for debt in &open_debts {
            if debt.relationship_soil.trim().is_empty() {
                results.push(DiagnosticResult {
                    id: next_id(),
                    severity: DiagnosticSeverity::Info,
                    category: DiagnosticCategory::PayoffGap,
                    message: format!(
                        "情绪债务 '{}' 缺少关系土壤。情绪债务需要人物关系作为催生基础。",
                        debt.title
                    ),
                    entity_name: Some(debt.title.clone()),
                    from: 0,
                    to: 0,
                    evidence: vec![],
                    fix_suggestion: Some(format!(
                        "明确 '{}' 的情绪来源关系（师徒/亲密/敌对/家族等）",
                        debt.title
                    )),
                    operations: vec![],
                });
            }
        }

        results
    }
}
