use serde::{Deserialize, Serialize};

/// Semantic intents the router can classify
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Chat,
    RetrieveKnowledge,
    AnalyzeText,
    GenerateContent,
    ExecutePlan,
    Linter,
}

/// Lightweight intent classifier — regex/pattern-based, no LLM call
pub fn classify_intent(input: &str, has_lorebook: bool, has_outline: bool) -> Intent {
    let lower = input.to_lowercase();

    // Plan execution keywords
    let plan_keywords = [
        "outline",
        "大纲",
        "generate all",
        "batch",
        "according to the outline",
        "根据大纲",
        "全部生成",
    ];
    if plan_keywords.iter().any(|k| lower.contains(k)) && has_outline {
        return Intent::ExecutePlan;
    }

    // Knowledge retrieval keywords
    let lore_keywords = [
        "who is",
        "what is",
        "where is",
        "tell me about",
        "谁是",
        "什么是",
        "在哪里",
        "查",
        "设定",
        "lorebook",
        "character",
    ];
    if lore_keywords.iter().any(|k| lower.contains(k)) && has_lorebook {
        return Intent::RetrieveKnowledge;
    }

    // Analysis keywords
    let analyze_keywords = [
        "analyze",
        "review",
        "check",
        "find issues",
        "pacing",
        "plot hole",
        "分析",
        "审查",
        "检查",
        "找问题",
        "节奏",
        "漏洞",
    ];
    if analyze_keywords.iter().any(|k| lower.contains(k)) {
        return Intent::AnalyzeText;
    }

    // Content generation keywords
    let generate_keywords = [
        "write",
        "draft",
        "continue",
        "expand",
        "generate",
        "create",
        "写",
        "续写",
        "展开",
        "生成",
        "创作",
        "写一段",
        "写一章",
    ];
    if generate_keywords.iter().any(|k| lower.contains(k)) {
        return Intent::GenerateContent;
    }

    Intent::Chat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify() {
        assert_eq!(classify_intent("hello", true, true), Intent::Chat);
        assert_eq!(
            classify_intent("who is 林墨?", true, true),
            Intent::RetrieveKnowledge
        );
        assert_eq!(
            classify_intent("analyze my chapter", true, true),
            Intent::AnalyzeText
        );
        assert_eq!(
            classify_intent("write a fight scene", true, true),
            Intent::GenerateContent
        );
    }
}
