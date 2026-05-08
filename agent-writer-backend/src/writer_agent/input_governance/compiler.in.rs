use super::super::memory::WriterMemory;
use super::CompiledInput;
use crate::agent_runtime;

pub fn compile_input(
    memory: &WriterMemory,
    project_id: &str,
    chapter_title: &str,
    _user_instruction: &str,
) -> CompiledInput {
    let mut evidence = Vec::new();
    let mut rules = Vec::new();

    // Get chapter mission as intent
    let intent = memory
        .get_chapter_mission(project_id, chapter_title)
        .ok()
        .flatten()
        .map(|m| m.mission)
        .unwrap_or_else(|| format!("Write chapter {}", chapter_title));

    // Collect character states as evidence
    if let Ok(chars) = memory.list_characters(None) {
        for c in chars.iter().take(5) {
            if let Ok(Some(state)) = memory.get_active_state(c.id, chapter_title) {
                if let Some(goals) = state.goal_state.as_object() {
                    for (k, v) in goals {
                        evidence.push(format!("{}: {}={}", c.name, k, v));
                    }
                }
            }
        }
    }

    // Collect active knowledge as evidence
    if let Ok(items) = memory.list_knowledge_items(Some("objective")) {
        for item in items.iter().take(3) {
            evidence.push(format!("knowledge:{}", item.topic));
        }
    }

    // Collect canon rules
    if let Ok(canon_rules) = memory.list_canon_rules(5) {
        for rule in &canon_rules {
            rules.push(format!("{}: {}", rule.category, rule.rule));
        }
    }

    CompiledInput {
        intent_text: intent,
        selected_evidence: evidence,
        rule_stack: rules,
        trace_hint: format!("compiled:{}", chapter_title),
        compiled_at_ms: agent_runtime::now_ms(),
    }
}
