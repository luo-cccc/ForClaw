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

/// Structured classification result with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentClassification {
    pub intent: Intent,
    pub confidence: f32,                 // 0.0-1.0
    pub reason: String,                  // why this intent was chosen
    pub evidence: Vec<String>,           // keywords that matched
    pub fallback_intent: Option<Intent>, // alternative if confidence low
    pub source: ClassificationSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationSource {
    KeywordMatch,
    Fallback,
    Default,
}

/// Lightweight intent classifier with confidence, evidence, and fallback.
///
/// Returns an `IntentClassification` containing the best intent, confidence
/// score (0-1), matched keyword evidence, fallback alternative for low-
/// confidence cases, and the classification source.
pub fn classify_intent(input: &str, has_lorebook: bool, has_outline: bool) -> IntentClassification {
    let lower = input.to_lowercase();

    // Define keyword groups with weights
    let plan_matches = keyword_matches(
        &lower,
        &[
            ("outline", 1.0),
            ("大纲", 1.0),
            ("generate all", 0.9),
            ("batch", 0.7),
            ("according to the outline", 0.9),
            ("根据大纲", 1.0),
            ("全部生成", 0.9),
        ],
    );

    let lore_matches = keyword_matches(
        &lower,
        &[
            ("who is", 0.8),
            ("what is", 0.7),
            ("tell me about", 0.7),
            ("谁是", 1.0),
            ("什么是", 1.0),
            ("在哪里", 0.8),
            ("查", 0.6),
            ("设定", 0.9),
            ("lorebook", 0.9),
            ("character", 0.6),
        ],
    );

    let analyze_matches = keyword_matches(
        &lower,
        &[
            ("analyze", 0.9),
            ("review", 0.7),
            ("check", 0.6),
            ("find issues", 0.8),
            ("pacing", 0.8),
            ("plot hole", 0.9),
            ("分析", 1.0),
            ("审查", 0.9),
            ("检查", 0.7),
            ("找问题", 0.9),
            ("节奏", 0.8),
            ("漏洞", 0.9),
        ],
    );

    let generate_matches = keyword_matches(
        &lower,
        &[
            ("write", 0.8),
            ("draft", 0.8),
            ("continue", 0.7),
            ("expand", 0.6),
            ("generate", 0.8),
            ("create", 0.6),
            ("写", 0.9),
            ("续写", 0.9),
            ("展开", 0.7),
            ("生成", 0.8),
            ("创作", 0.8),
            ("写一段", 0.9),
            ("写一章", 0.9),
        ],
    );

    // Score each intent
    let plan_score = intent_score(&plan_matches, has_outline, 1.0, 0.2);
    let lore_score = intent_score(&lore_matches, has_lorebook, 0.9, 0.1);
    let analyze_score = intent_score(&analyze_matches, true, 0.8, 0.1);
    let generate_score = intent_score(&generate_matches, true, 0.7, 0.1);

    let chat_score = 0.15; // chat is the default, low confidence

    // Assemble scores with evidence
    let scores: Vec<(Intent, f32, Vec<String>)> = vec![
        (
            Intent::ExecutePlan,
            plan_score,
            plan_matches.iter().map(|(k, _)| k.to_string()).collect(),
        ),
        (
            Intent::RetrieveKnowledge,
            lore_score,
            lore_matches.iter().map(|(k, _)| k.to_string()).collect(),
        ),
        (
            Intent::AnalyzeText,
            analyze_score,
            analyze_matches.iter().map(|(k, _)| k.to_string()).collect(),
        ),
        (
            Intent::GenerateContent,
            generate_score,
            generate_matches
                .iter()
                .map(|(k, _)| k.to_string())
                .collect(),
        ),
        (Intent::Chat, chat_score, vec![]),
    ];

    // Find best and second-best
    let mut sorted: Vec<(Intent, f32, Vec<String>)> = scores;
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let primary = &sorted[0];
    let fallback = &sorted[1];

    let confidence = primary.1;
    let source = if confidence >= 0.5 {
        ClassificationSource::KeywordMatch
    } else if confidence >= 0.2 {
        ClassificationSource::Fallback
    } else {
        ClassificationSource::Default
    };

    IntentClassification {
        intent: primary.0.clone(),
        confidence,
        reason: if primary.2.is_empty() {
            "no keywords matched, defaulted to Chat".to_string()
        } else {
            format!("matched keywords: {}", primary.2.join(", "))
        },
        evidence: primary.2.clone(),
        fallback_intent: if confidence < 0.5 {
            Some(fallback.0.clone())
        } else {
            None
        },
        source,
    }
}

fn keyword_matches<'a>(text: &str, keywords: &'a [(&str, f32)]) -> Vec<(&'a str, f32)> {
    keywords
        .iter()
        .filter(|(kw, _)| text.contains(kw))
        .map(|(kw, weight)| (*kw, *weight))
        .collect()
}

fn intent_score(
    matches: &[(&str, f32)],
    has_precondition: bool,
    precondition_weight: f32,
    baseline: f32,
) -> f32 {
    if matches.is_empty() {
        return baseline * if has_precondition { 0.5 } else { 0.0 };
    }
    let raw: f32 = matches.iter().map(|(_, w)| w).sum();
    let boosted = if has_precondition {
        (raw * precondition_weight).min(0.95)
    } else {
        raw * 0.3
    };
    boosted.max(baseline)
}

/// Backward-compatible simple classifier returning only the `Intent`.
pub fn classify_intent_simple(input: &str, has_lorebook: bool, has_outline: bool) -> Intent {
    classify_intent(input, has_lorebook, has_outline).intent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify() {
        assert_eq!(classify_intent("hello", true, true).intent, Intent::Chat);
        assert_eq!(
            classify_intent("who is 林墨?", true, true).intent,
            Intent::RetrieveKnowledge
        );
        assert_eq!(
            classify_intent("analyze my chapter", true, true).intent,
            Intent::AnalyzeText
        );
        assert_eq!(
            classify_intent("write a fight scene", true, true).intent,
            Intent::GenerateContent
        );
    }

    #[test]
    fn test_classify_with_confidence() {
        let result = classify_intent("write a fight scene", true, true);
        assert_eq!(result.intent, Intent::GenerateContent);
        assert!(result.confidence > 0.5);
        assert!(!result.evidence.is_empty());
    }

    #[test]
    fn test_ambiguous_input_has_fallback() {
        let result = classify_intent("hello", true, true);
        assert_eq!(result.source, ClassificationSource::Default);
        // Low confidence => fallback should be set
        if result.confidence < 0.5 {
            assert!(result.fallback_intent.is_some());
        }
    }

    #[test]
    fn test_chinese_keywords_higher_confidence() {
        let en = classify_intent("generate a chapter", true, true);
        let zh = classify_intent("写一章", true, true);
        // Chinese keyword weights are higher, so ZH should have >= EN confidence
        assert!(zh.confidence >= en.confidence);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let result = classify_intent("分析本章的节奏问题", true, true);
        let json = serde_json::to_string(&result).unwrap();
        let decoded: IntentClassification = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.intent, Intent::AnalyzeText);
    }

    #[test]
    fn test_backward_compat_simple() {
        assert_eq!(classify_intent_simple("hello", true, true), Intent::Chat);
        assert_eq!(
            classify_intent_simple("write a fight scene", true, true),
            Intent::GenerateContent
        );
    }

    #[test]
    fn test_classification_reason_not_empty() {
        let result = classify_intent("写一章", true, true);
        assert!(!result.reason.is_empty());
        // Even default chat has a reason
        let chat = classify_intent("ok", true, true);
        assert!(!chat.reason.is_empty());
    }
}
