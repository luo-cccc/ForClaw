//! IntentEngine — stable author-intent detection.
//! Expands the keyword classifier with structured output and testable rules.

use serde::{Deserialize, Serialize};

/// Writing intent taxonomy — what the author is doing right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WritingIntent {
    Dialogue,
    Action,
    Description,
    Transition,
    Exposition,
    EmotionalBeat,
    ConflictEscalation,
    Reveal,
    SetupOrPayoff,
    Revision,
    StructuralPlanning,
    CanonMaintenance,
}

/// Desired agent behavior based on intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentBehavior {
    StaySilent,
    SuggestContinuation,
    OfferRevision,
    WarnContinuity,
    ProposeStructure,
    MaintainCanon,
    GenerateDraft,
}

/// Structured intent estimate with confidence and cues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingIntentEstimate {
    pub primary: WritingIntent,
    pub secondary: Vec<WritingIntent>,
    pub confidence: f32,
    pub cues: Vec<String>,
    pub desired_behavior: AgentBehavior,
}

/// Intent classification rules — deterministic, testable.
pub struct IntentEngine {
    rules: Vec<IntentRule>,
}

struct IntentRule {
    intent: WritingIntent,
    cues: Vec<&'static str>,
    behavior: AgentBehavior,
    min_confidence: f32,
}

impl IntentEngine {
    pub fn new() -> Self {
        Self {
            rules: vec![
                IntentRule {
                    intent: WritingIntent::Dialogue,
                    cues: vec![
                        "\"", "「", "」", "\u{201c}", "\u{201d}", "说", "问道", "回答", "喊道",
                        "低语", "喃喃", "道",
                    ],
                    behavior: AgentBehavior::SuggestContinuation,
                    min_confidence: 0.15,
                },
                IntentRule {
                    intent: WritingIntent::Action,
                    cues: vec![
                        "拔", "挥", "冲", "跳", "踢", "打", "击", "闪", "避", "刺", "劈", "砍",
                    ],
                    behavior: AgentBehavior::SuggestContinuation,
                    min_confidence: 0.15,
                },
                IntentRule {
                    intent: WritingIntent::Description,
                    cues: vec![
                        "破庙", "密道", "山", "林", "城", "宫殿", "剑", "刀", "雾", "月光", "烛",
                        "窗",
                    ],
                    behavior: AgentBehavior::SuggestContinuation,
                    min_confidence: 0.08,
                },
                IntentRule {
                    intent: WritingIntent::EmotionalBeat,
                    cues: vec![
                        "愤怒", "悲伤", "恐惧", "喜悦", "沉默", "泪", "颤抖", "心跳", "握紧",
                        "咬牙",
                    ],
                    behavior: AgentBehavior::StaySilent,
                    min_confidence: 0.1,
                },
                IntentRule {
                    intent: WritingIntent::ConflictEscalation,
                    cues: vec![
                        "突然",
                        "但是",
                        "然而",
                        "不料",
                        "没想到",
                        "竟然",
                        "猛地",
                        "瞬间",
                    ],
                    behavior: AgentBehavior::SuggestContinuation,
                    min_confidence: 0.12,
                },
                IntentRule {
                    intent: WritingIntent::Revision,
                    cues: vec!["selected_text"],
                    behavior: AgentBehavior::OfferRevision,
                    min_confidence: 0.3,
                },
                IntentRule {
                    intent: WritingIntent::StructuralPlanning,
                    cues: vec!["outline_active", "chapter_switch"],
                    behavior: AgentBehavior::ProposeStructure,
                    min_confidence: 0.5,
                },
                IntentRule {
                    intent: WritingIntent::CanonMaintenance,
                    cues: vec!["lorebook_edit", "character_sheet"],
                    behavior: AgentBehavior::MaintainCanon,
                    min_confidence: 0.5,
                },
            ],
        }
    }

    /// Classify writing intent from paragraph text and observation metadata.
    pub fn classify(
        &self,
        paragraph: &str,
        has_selection: bool,
        is_chapter_switch: bool,
    ) -> WritingIntentEstimate {
        let mut scores: Vec<(WritingIntent, f32, Vec<&str>, &AgentBehavior)> = Vec::new();

        for rule in &self.rules {
            let mut matches = 0u32;
            let mut matched_cues = Vec::new();

            for cue in &rule.cues {
                // Meta-cues
                if *cue == "selected_text" && has_selection {
                    matches += 3;
                    matched_cues.push("selected_text");
                    continue;
                }
                if *cue == "chapter_switch" && is_chapter_switch {
                    matches += 2;
                    matched_cues.push("chapter_switch");
                    continue;
                }
                // Text cues
                if paragraph.contains(cue) {
                    matches += 1;
                    matched_cues.push(*cue);
                }
            }

            if matches > 0 {
                let confidence = (matches as f32 / rule.cues.len().max(1) as f32).min(1.0);
                if confidence >= rule.min_confidence {
                    scores.push((
                        rule.intent.clone(),
                        confidence,
                        matched_cues,
                        &rule.behavior,
                    ));
                }
            }
        }

        // Sort by confidence descending
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if scores.is_empty() {
            return WritingIntentEstimate {
                primary: WritingIntent::Description,
                secondary: vec![],
                confidence: 0.3,
                cues: vec!["fallback".into()],
                desired_behavior: AgentBehavior::SuggestContinuation,
            };
        }

        let primary = scores[0].0.clone();
        let cues: Vec<String> = scores[0].2.iter().map(|s| s.to_string()).collect();
        let behavior = scores[0].3.clone();

        let secondary: Vec<WritingIntent> =
            scores.iter().skip(1).take(2).map(|s| s.0.clone()).collect();

        WritingIntentEstimate {
            primary,
            secondary,
            confidence: scores[0].1,
            cues,
            desired_behavior: behavior,
        }
    }
}

impl Default for IntentEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialogue_detection() {
        let engine = IntentEngine::new();
        let est = engine.classify(
            "\"你不能这样做，\"她低声说道，\"已经太晚了。\"",
            false,
            false,
        );
        assert_eq!(est.primary, WritingIntent::Dialogue);
        assert!(est.confidence > 0.15);
    }

    #[test]
    fn test_action_detection() {
        let engine = IntentEngine::new();
        let est = engine.classify("林墨拔出长剑，猛地冲向敌人，一剑劈下", false, false);
        // Has both action (拔,冲,击,劈) and conflict (猛地) cues — either is valid
        let is_action_or_conflict = est.primary == WritingIntent::Action
            || est.primary == WritingIntent::ConflictEscalation;
        assert!(
            is_action_or_conflict,
            "expected action or conflict, got {:?}",
            est.primary
        );
    }

    #[test]
    fn test_revision_with_selection() {
        let engine = IntentEngine::new();
        let est = engine.classify("一些文本", true, false);
        assert_eq!(est.primary, WritingIntent::Revision);
        assert!(est.confidence >= 0.7);
    }

    #[test]
    fn test_emotional_beat_stays_silent() {
        let engine = IntentEngine::new();
        let est = engine.classify("她沉默着，眼泪无声滑落，手指微微颤抖", false, false);
        assert_eq!(est.primary, WritingIntent::EmotionalBeat);
        assert_eq!(est.desired_behavior, AgentBehavior::StaySilent);
    }

    #[test]
    fn test_fallback_on_empty() {
        let engine = IntentEngine::new();
        let est = engine.classify("平凡的文字", false, false);
        assert!(est.confidence <= 0.31);
    }

    #[test]
    fn test_conflict_escalation() {
        let engine = IntentEngine::new();
        let est = engine.classify("突然，一阵狂风袭来，不料竟暗藏杀机", false, false);
        assert_eq!(est.primary, WritingIntent::ConflictEscalation);
    }
}
