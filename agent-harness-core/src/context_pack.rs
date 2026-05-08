use serde::{Deserialize, Serialize};

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
        });
    }

    pub fn finish(self) -> PackedContext {
        let included_chars = char_count(&self.text);
        let truncated_source_count = self
            .sources
            .iter()
            .filter(|source| source.truncated)
            .count();
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
        }
    }
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
}
