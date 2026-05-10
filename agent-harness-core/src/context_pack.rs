use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceReport {
    pub source_type: String,
    pub id: String,
    pub label: String,
    pub original_chars: usize,
    pub included_chars: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    // P1: source taxonomy, timing and retrieval status
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub taxonomy: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default)]
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub retrieval_status: String,
}

// P1: stable source taxonomy constants
pub const TAXONOMY_OUTLINE: &str = "outline";
pub const TAXONOMY_LORE: &str = "lore";
pub const TAXONOMY_PRIOR_CHAPTER: &str = "prior_chapter";
pub const TAXONOMY_PROJECT_BRAIN: &str = "project_brain";
pub const TAXONOMY_PROMISE: &str = "promise";
pub const TAXONOMY_CANON: &str = "canon";
pub const TAXONOMY_MEMORY: &str = "memory";
pub const TAXONOMY_INSTRUCTION: &str = "instruction";
pub const TAXONOMY_AUTHOR_VOICE: &str = "author_voice";
pub const TAXONOMY_SCENE_PLAN: &str = "scene_plan";
pub const TAXONOMY_UNKNOWN: &str = "unknown";

/// Priority for deterministic source ordering (lower = higher priority).
/// Core grounding sources first, volatile/auxiliary sources last.
pub fn source_priority(source_type: &str) -> usize {
    match source_type {
        "instruction" | "system_contract" => 0,
        "outline" | "target_beat" => 10,
        "previous_chapters" | "prior_chapter" => 20,
        "lorebook" | "canon" | "promise" => 30,
        "project_brain" | "rag" => 40,
        "next_chapter" => 50,
        "target_existing_text" => 60,
        "user_profile" | "author_style" => 70,
        "story_impact" | "reader_compensation" => 80,
        _ => 90,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudgetReport {
    pub max_chars: usize,
    pub included_chars: usize,
    pub source_count: usize,
    pub truncated_source_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackedContext {
    pub text: String,
    pub sources: Vec<ContextSourceReport>,
    pub budget: ContextBudgetReport,
    /// Deterministic hash of all source contents concatenated.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub context_hash: String,
}

#[derive(Debug, Clone)]
pub struct ContextPacker {
    max_chars: usize,
    text: String,
    sources: Vec<ContextSourceReport>,
    warnings: Vec<String>,
}

impl ContextPacker {
    pub fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            text: String::new(),
            sources: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn remaining_chars(&self) -> usize {
        self.max_chars.saturating_sub(char_count(&self.text))
    }

    pub fn add_source(
        &mut self,
        source_type: &str,
        id: &str,
        label: &str,
        content: &str,
        source_cap: usize,
        score: Option<f32>,
    ) {
        self.add_source_with_meta(
            source_type,
            id,
            label,
            content,
            source_cap,
            score,
            "",
            "",
            0,
            "",
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_source_with_meta(
        &mut self,
        source_type: &str,
        id: &str,
        label: &str,
        content: &str,
        source_cap: usize,
        score: Option<f32>,
        taxonomy: &str,
        role: &str,
        elapsed_ms: u64,
        retrieval_status: &str,
    ) {
        if content.trim().is_empty() || self.remaining_chars() == 0 {
            return;
        }

        let header = format!("## {}\n", label);
        let footer = "\n\n";
        let overhead = char_count(&header) + char_count(footer);
        let remaining = self.remaining_chars();
        if remaining <= overhead {
            self.warnings
                .push(format!("Context budget exhausted before adding {}.", label));
            return;
        }

        let allowed = source_cap.min(remaining - overhead);
        let original_chars = char_count(content);
        let (included, included_chars, truncated) = truncate_text_report(content, allowed);

        self.text.push_str(&header);
        self.text.push_str(&included);
        self.text.push_str(footer);

        if truncated {
            self.warnings.push(format!(
                "{} truncated from {} to {} chars.",
                label, original_chars, included_chars
            ));
        }

        self.sources.push(ContextSourceReport {
            source_type: source_type.to_string(),
            id: id.to_string(),
            label: label.to_string(),
            original_chars,
            included_chars,
            truncated,
            score,
            taxonomy: taxonomy.to_string(),
            role: role.to_string(),
            elapsed_ms,
            retrieval_status: retrieval_status.to_string(),
        });
    }

    pub fn finish(mut self) -> PackedContext {
        let included_chars = char_count(&self.text);
        let truncated_source_count = self
            .sources
            .iter()
            .filter(|source| source.truncated)
            .count();

        // Deterministic ordering by source priority, then by id for stability.
        self.sources.sort_by(|a, b| {
            let pa = source_priority(&a.source_type);
            let pb = source_priority(&b.source_type);
            pa.cmp(&pb).then_with(|| a.id.cmp(&b.id))
        });

        let context_hash = compute_context_hash(&self.sources);

        PackedContext {
            text: self.text,
            budget: ContextBudgetReport {
                max_chars: self.max_chars,
                included_chars,
                source_count: self.sources.len(),
                truncated_source_count,
                warnings: self.warnings,
            },
            sources: self.sources,
            context_hash,
        }
    }
}

/// Compute a deterministic hash from all source contents concatenated.
pub fn compute_context_hash(sources: &[ContextSourceReport]) -> String {
    let mut hasher = DefaultHasher::new();
    for source in sources {
        source.source_type.hash(&mut hasher);
        source.id.hash(&mut hasher);
        source.label.hash(&mut hasher);
        source.included_chars.hash(&mut hasher);
        source.original_chars.hash(&mut hasher);
        source.truncated.hash(&mut hasher);
        source.taxonomy.hash(&mut hasher);
        source.role.hash(&mut hasher);
        source.elapsed_ms.hash(&mut hasher);
        source.retrieval_status.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

pub fn char_count(text: &str) -> usize {
    text.chars().count()
}

pub fn truncate_text_report(text: &str, max_chars: usize) -> (String, usize, bool) {
    let original_chars = char_count(text);
    if original_chars <= max_chars {
        return (text.to_string(), original_chars, false);
    }

    let truncated = text.chars().take(max_chars).collect::<String>();
    let included_chars = char_count(&truncated);
    (truncated, included_chars, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packer_respects_per_source_and_total_budget() {
        let mut packer = ContextPacker::new(42);
        packer.add_source("chapter", "one", "Chapter One", "林墨推门而入", 3, None);
        packer.add_source("lore", "rule", "Rule", "abcdef", 6, Some(0.8));

        let packed = packer.finish();
        assert!(packed.budget.included_chars <= 42);
        assert_eq!(packed.sources[0].included_chars, 3);
        assert!(packed.sources[0].truncated);
        assert_eq!(packed.budget.truncated_source_count, 1);
    }

    #[test]
    fn truncate_report_counts_unicode_chars() {
        let (text, included, truncated) = truncate_text_report("林墨推门", 2);
        assert_eq!(text, "林墨");
        assert_eq!(included, 2);
        assert!(truncated);
    }

    #[test]
    fn same_input_produces_deterministic_order_and_hash() {
        let mut packer_a = ContextPacker::new(200);
        packer_a.add_source_with_meta(
            "lorebook",
            "l1",
            "Lore",
            "content-a",
            50,
            None,
            TAXONOMY_LORE,
            "grounding",
            10,
            "ok",
        );
        packer_a.add_source_with_meta(
            "instruction",
            "i1",
            "Instruction",
            "content-b",
            50,
            None,
            TAXONOMY_INSTRUCTION,
            "directive",
            5,
            "ok",
        );
        packer_a.add_source_with_meta(
            "outline",
            "o1",
            "Outline",
            "content-c",
            50,
            None,
            TAXONOMY_OUTLINE,
            "grounding",
            8,
            "ok",
        );

        let mut packer_b = ContextPacker::new(200);
        // Add in reverse order
        packer_b.add_source_with_meta(
            "outline",
            "o1",
            "Outline",
            "content-c",
            50,
            None,
            TAXONOMY_OUTLINE,
            "grounding",
            8,
            "ok",
        );
        packer_b.add_source_with_meta(
            "instruction",
            "i1",
            "Instruction",
            "content-b",
            50,
            None,
            TAXONOMY_INSTRUCTION,
            "directive",
            5,
            "ok",
        );
        packer_b.add_source_with_meta(
            "lorebook",
            "l1",
            "Lore",
            "content-a",
            50,
            None,
            TAXONOMY_LORE,
            "grounding",
            10,
            "ok",
        );

        let packed_a = packer_a.finish();
        let packed_b = packer_b.finish();

        // Same hash despite different insertion order
        assert_eq!(packed_a.context_hash, packed_b.context_hash);
        // Sources ordered by priority: instruction (0) < outline (10) < lorebook (30)
        assert_eq!(packed_a.sources[0].source_type, "instruction");
        assert_eq!(packed_a.sources[1].source_type, "outline");
        assert_eq!(packed_a.sources[2].source_type, "lorebook");
        assert_eq!(packed_b.sources[0].source_type, "instruction");
        assert_eq!(packed_b.sources[1].source_type, "outline");
        assert_eq!(packed_b.sources[2].source_type, "lorebook");
    }

    #[test]
    fn source_timeout_does_not_swallow_other_sources() {
        let mut packer = ContextPacker::new(200);
        packer.add_source_with_meta(
            "instruction",
            "i1",
            "Instruction",
            "content-b",
            50,
            None,
            TAXONOMY_INSTRUCTION,
            "directive",
            5,
            "ok",
        );
        // Simulate a failed/timeout source with empty content — it should be skipped
        packer.add_source_with_meta(
            "project_brain",
            "p1",
            "RAG",
            "",
            50,
            None,
            TAXONOMY_PROJECT_BRAIN,
            "memory",
            5000,
            "timeout",
        );
        packer.add_source_with_meta(
            "outline",
            "o1",
            "Outline",
            "content-c",
            50,
            None,
            TAXONOMY_OUTLINE,
            "grounding",
            8,
            "ok",
        );

        let packed = packer.finish();
        assert_eq!(packed.sources.len(), 2);
        assert!(packed
            .sources
            .iter()
            .any(|s| s.source_type == "instruction"));
        assert!(packed.sources.iter().any(|s| s.source_type == "outline"));
        assert!(!packed
            .sources
            .iter()
            .any(|s| s.source_type == "project_brain"));
    }

    #[test]
    fn context_hash_changes_when_source_content_changes() {
        let mut packer_a = ContextPacker::new(200);
        packer_a.add_source_with_meta(
            "instruction",
            "i1",
            "Instruction",
            "content-b",
            50,
            None,
            TAXONOMY_INSTRUCTION,
            "directive",
            5,
            "ok",
        );
        let packed_a = packer_a.finish();

        let mut packer_b = ContextPacker::new(200);
        packer_b.add_source_with_meta(
            "instruction",
            "i1",
            "Instruction",
            "different-content",
            50,
            None,
            TAXONOMY_INSTRUCTION,
            "directive",
            5,
            "ok",
        );
        let packed_b = packer_b.finish();

        assert_ne!(packed_a.context_hash, packed_b.context_hash);
    }
}
