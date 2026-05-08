//! CanonEngine — protects story truth.
//! Detects entity mentions and checks against canon facts.

use super::memory::WriterMemory;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct CanonEngine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonCheck {
    pub entity_name: String,
    pub mentioned_attribute: String,
    pub mentioned_value: String,
    pub canon_value: Option<String>,
    pub conflict: bool,
    pub confidence: f64,
}

impl Default for CanonEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CanonEngine {
    pub fn new() -> Self {
        Self
    }

    /// Check a paragraph for canon conflicts.
    /// Returns all detected conflicts for review.
    pub fn check_paragraph(&self, paragraph: &str, memory: &WriterMemory) -> Vec<CanonCheck> {
        let mut checks = Vec::new();
        // Simple entity+attribute heuristic — enhanced by LLM in full pipeline
        let entity_weapon_patterns = [
            ("拔出", "weapon"),
            ("举起", "weapon"),
            ("挥动", "weapon"),
            ("抽出了", "weapon"),
            ("握着", "weapon"),
        ];

        let known_names = memory.get_canon_entity_names().unwrap_or_default();

        for (action, attr) in &entity_weapon_patterns {
            if let Some(pos) = paragraph.find(action) {
                let after: String = paragraph[pos + action.len()..].chars().take(20).collect();
                // Try to find the weapon mentioned
                for weapon in &["剑", "刀", "枪", "弓", "匕首", "棍", "鞭", "斧"] {
                    if after.contains(weapon) {
                        // Check canon for nearby entity names
                        if let Some(entity_name) = find_entity_before(paragraph, pos, &known_names)
                        {
                            if let Ok(facts) = memory.get_canon_facts_for_entity(&entity_name) {
                                if let Some((_, canon_weapon)) =
                                    facts.iter().find(|(k, _)| k == attr)
                                {
                                    let conflict = canon_weapon != weapon;
                                    checks.push(CanonCheck {
                                        entity_name,
                                        mentioned_attribute: attr.to_string(),
                                        mentioned_value: weapon.to_string(),
                                        canon_value: Some(canon_weapon.clone()),
                                        conflict,
                                        confidence: if conflict { 0.85 } else { 0.95 },
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        checks
    }
}

fn find_entity_before(text: &str, pos: usize, known_names: &[String]) -> Option<String> {
    let before: String = text[..pos].chars().collect();
    for name in known_names {
        if before.ends_with(name) || before.contains(name) {
            return Some(name.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canon_check_no_conflict_when_no_facts() {
        let m = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let engine = CanonEngine::new();
        let checks = engine.check_paragraph("林墨拔出一把长剑，指向敌人", &m);
        assert!(checks.is_empty() || !checks.iter().any(|c| c.conflict));
    }

    #[test]
    fn test_canon_check_detects_entity_pattern() {
        let m = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        m.upsert_canon_entity(
            "location",
            "林墨",
            &[],
            "主角",
            &serde_json::json!({"weapon": "寒影刀"}),
            0.9,
        )
        .unwrap();
        let engine = CanonEngine::new();
        let checks = engine.check_paragraph("林墨拔出长剑", &m);
        assert!(checks
            .iter()
            .any(|c| c.mentioned_value == "剑" && c.conflict));
    }
}
