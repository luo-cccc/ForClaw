#[cfg(test)]
mod tests {
    use super::super::memory::WriterMemory;
    use super::*;

    fn test_memory() -> WriterMemory {
        WriterMemory::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_extract_entities_action() {
        let m = test_memory();
        let entities = extract_entities("林墨拔出一把长剑", &m);
        assert!(entities.contains(&"林墨".to_string()));
    }

    #[test]
    fn test_detect_weapon_value() {
        let val = detect_attribute_value("林墨拔出一把长剑指向天空", "林墨", "weapon");
        assert_eq!(val, Some("长剑".to_string()));
    }

    #[test]
    fn test_diagnose_weapon_conflict() {
        let m = test_memory();
        m.upsert_canon_entity(
            "location",
            "林墨",
            &[],
            "主角",
            &serde_json::json!({"weapon": "寒影刀"}),
            0.9,
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose("林墨拔出一把长剑", 10, "ch3", "default", &m);
        let conflict = results
            .iter()
            .find(|result| matches!(result.category, DiagnosticCategory::CanonConflict))
            .unwrap();
        assert_eq!(conflict.from, 16);
        assert_eq!(conflict.to, 18);
    }

    #[test]
    fn test_diagnose_accepts_weapon_family_match() {
        let m = test_memory();
        m.upsert_canon_entity(
            "location",
            "林墨",
            &[],
            "主角",
            &serde_json::json!({"weapon": "寒影刀"}),
            0.9,
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose("林墨拔出寒影刀", 0, "Chapter-3", "default", &m);
        assert!(!results
            .iter()
            .any(|result| matches!(result.category, DiagnosticCategory::CanonConflict)));
    }

    #[test]
    fn test_chapter_number_order_avoids_lexicographic_regression() {
        assert!(is_later_chapter("Chapter-10", "Chapter-2"));
        assert!(!is_later_chapter("Chapter-2", "Chapter-10"));
    }

    #[test]
    fn test_promise_opportunity_uses_terms_not_fixed_prefix() {
        let m = test_memory();
        m.add_promise(
            "object_in_motion",
            "玉佩",
            "张三拿走玉佩，需要交代下落",
            "Chapter-1",
            "Chapter-4",
            4,
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose("张三把那枚玉佩放回桌上。", 0, "Chapter-3", "default", &m);
        assert!(results
            .iter()
            .any(|result| matches!(result.category, DiagnosticCategory::UnresolvedPromise)));
    }

    #[test]
    fn test_promise_not_flagged_from_future_chapter() {
        let m = test_memory();
        m.add_promise(
            "object_in_motion",
            "玉佩",
            "张三拿走玉佩，需要交代下落",
            "Chapter-10",
            "Chapter-12",
            4,
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose("张三把那枚玉佩放回桌上。", 0, "Chapter-2", "default", &m);
        assert!(!results
            .iter()
            .any(|result| matches!(result.category, DiagnosticCategory::UnresolvedPromise)));
    }

    #[test]
    fn test_stale_promise_warns_at_payoff_chapter() {
        let m = test_memory();
        m.add_promise(
            "mystery",
            "密道",
            "破庙里有密道，需要揭示用途",
            "Chapter-1",
            "Chapter-3",
            5,
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose(
            "林墨推开门，雨声压住了脚步。",
            0,
            "Chapter-3",
            "default",
            &m,
        );
        assert!(results.iter().any(|result| {
            matches!(result.category, DiagnosticCategory::UnresolvedPromise)
                && matches!(result.severity, DiagnosticSeverity::Warning)
        }));
    }

    #[test]
    fn test_story_contract_boundary_violation_warns() {
        let m = test_memory();
        m.ensure_story_contract_seed(
            "default",
            "寒影录",
            "玄幻",
            "刀客追查玉佩真相。",
            "林墨必须在复仇和守护之间做选择。",
            "不得提前泄露玉佩来源。",
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose(
            "张三终于说出真相：玉佩其实来自禁地。",
            0,
            "Chapter-2",
            "default",
            &m,
        );
        let warning = results
            .iter()
            .find(|result| matches!(result.category, DiagnosticCategory::StoryContractViolation))
            .unwrap();
        assert!(warning.message.contains("书级合同违例"));
        assert_eq!(warning.evidence[0].source, "story_contract");
    }

    #[test]
    fn test_chapter_mission_must_not_violation_warns() {
        let m = test_memory();
        m.ensure_chapter_mission_seed(
            "default",
            "Chapter-2",
            "让林墨追查玉佩下落。",
            "玉佩线索",
            "提前揭开真相",
            "以误导线索收束。",
            "test",
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose(
            "林墨直接揭开了真相，玉佩来自禁地。",
            0,
            "Chapter-2",
            "default",
            &m,
        );
        let warning = results
            .iter()
            .find(|result| matches!(result.category, DiagnosticCategory::ChapterMissionViolation))
            .unwrap();
        assert!(warning.message.contains("章节任务违例"));
        assert_eq!(warning.evidence[0].source, "chapter_mission");
    }

    #[test]
    fn test_chapter_mission_must_not_negated_does_not_warn() {
        let m = test_memory();
        m.ensure_chapter_mission_seed(
            "default",
            "Chapter-2",
            "让林墨追查玉佩下落。",
            "玉佩线索",
            "提前揭开真相",
            "以误导线索收束。",
            "test",
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose(
            "林墨没有揭开真相，只把玉佩重新收进袖中。",
            0,
            "Chapter-2",
            "default",
            &m,
        );
        assert!(!results
            .iter()
            .any(|result| matches!(result.category, DiagnosticCategory::ChapterMissionViolation)));
    }

    #[test]
    fn test_story_contract_negated_reveal_does_not_warn() {
        let m = test_memory();
        m.ensure_story_contract_seed(
            "default",
            "寒影录",
            "玄幻",
            "刀客追查玉佩真相。",
            "林墨必须在复仇和守护之间做选择。",
            "不得提前泄露玉佩来源。",
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose(
            "张三没有说出真相，也不肯解释玉佩来源。",
            0,
            "Chapter-2",
            "default",
            &m,
        );
        assert!(!results
            .iter()
            .any(|result| matches!(result.category, DiagnosticCategory::StoryContractViolation)));
    }

    #[test]
    fn test_timeline_dead_character_action_warns() {
        let m = test_memory();
        m.upsert_canon_entity(
            "location",
            "张三",
            &[],
            "上一章已死亡",
            &serde_json::json!({"status": "已死亡"}),
            0.9,
        )
        .unwrap();
        let engine = DiagnosticsEngine::new();
        let results = engine.diagnose(
            "张三推门而入，说道：“我回来了。”",
            0,
            "Chapter-5",
            "default",
            &m,
        );
        assert!(results
            .iter()
            .any(|result| matches!(result.category, DiagnosticCategory::TimelineIssue)));
    }

    #[test]
    fn test_pacing_warning_long_paragraph() {
        let m = test_memory();
        let engine = DiagnosticsEngine::new();
        let long = "x".repeat(2001);
        let results = engine.diagnose(&long, 0, "ch1", "default", &m);
        assert!(results
            .iter()
            .any(|r| matches!(r.category, DiagnosticCategory::PacingNote)));
    }
}
