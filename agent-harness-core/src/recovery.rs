use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureBundle {
    pub run_id: String,
    pub failed_step: String,
    pub error_kind: String,
    pub completed_steps: Vec<String>,
    pub stuck_at: String,
    pub retry_parameters: Option<RetryParams>,
    pub suggested_action: RecoveryAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryParams {
    pub delay_ms: u64,
    pub max_context_chars: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Retry { delay_ms: u64 },
    ShrinkContext { max_chars: usize },
    ApprovalRequired { reason: String },
    Stop,
}

pub fn classify_failure(
    run_id: &str,
    error: &str,
    failed_step: &str,
    completed_steps: &[String],
) -> FailureBundle {
    let lower = error.to_ascii_lowercase();

    let (suggested_action, retry_parameters) =
        if lower.contains("rate limit") || lower.contains("429") {
            (
                RecoveryAction::Retry { delay_ms: 30000 },
                Some(RetryParams {
                    delay_ms: 30000,
                    max_context_chars: None,
                }),
            )
        } else if lower.contains("context")
            && (lower.contains("overflow") || lower.contains("too long"))
        {
            (
                RecoveryAction::ShrinkContext { max_chars: 16000 },
                Some(RetryParams {
                    delay_ms: 1000,
                    max_context_chars: Some(16000),
                }),
            )
        } else if lower.contains("timeout") || lower.contains("timed out") {
            (
                RecoveryAction::Retry { delay_ms: 15000 },
                Some(RetryParams {
                    delay_ms: 15000,
                    max_context_chars: None,
                }),
            )
        } else if lower.contains("approval") || lower.contains("budget") {
            (
                RecoveryAction::ApprovalRequired {
                    reason: error.to_string(),
                },
                None,
            )
        } else {
            (RecoveryAction::Stop, None)
        };

    let error_kind = if lower.contains("rate limit") || lower.contains("429") {
        "provider"
    } else if lower.contains("context") {
        "context_overflow"
    } else if lower.contains("approval") || lower.contains("budget") {
        "budget"
    } else {
        "backend"
    };

    FailureBundle {
        run_id: run_id.to_string(),
        failed_step: failed_step.to_string(),
        error_kind: error_kind.to_string(),
        completed_steps: completed_steps.to_vec(),
        stuck_at: failed_step.to_string(),
        retry_parameters,
        suggested_action,
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn rate_limit_suggests_retry() {
        let bundle = classify_failure(
            "r1",
            "LLM call failed (429): rate limited",
            "step-1",
            &["step-0".into()],
        );
        assert_eq!(
            bundle.suggested_action,
            RecoveryAction::Retry { delay_ms: 30000 }
        );
        assert_eq!(bundle.error_kind, "provider");
    }

    #[test]
    fn context_overflow_suggests_shrink() {
        let bundle = classify_failure("r2", "context_length_exceeded: context overflow", "step-2", &[]);
        assert_eq!(
            bundle.suggested_action,
            RecoveryAction::ShrinkContext { max_chars: 16000 }
        );
        assert_eq!(bundle.error_kind, "context_overflow");
    }

    #[test]
    fn timeout_suggests_retry() {
        let bundle = classify_failure("r3", "request timed out", "step-1", &[]);
        assert_eq!(
            bundle.suggested_action,
            RecoveryAction::Retry { delay_ms: 15000 }
        );
    }

    #[test]
    fn approval_error_suggests_approval() {
        let bundle = classify_failure("r4", "provider budget approval required", "step-3", &[]);
        assert!(matches!(
            bundle.suggested_action,
            RecoveryAction::ApprovalRequired { .. }
        ));
        assert_eq!(bundle.error_kind, "budget");
    }

    #[test]
    fn unknown_error_suggests_stop() {
        let bundle = classify_failure("r5", "something unexpected happened", "step-0", &[]);
        assert_eq!(bundle.suggested_action, RecoveryAction::Stop);
        assert_eq!(bundle.error_kind, "backend");
    }

    #[test]
    fn serialization_roundtrip() {
        let bundle = FailureBundle {
            run_id: "r1".into(),
            failed_step: "step-1".into(),
            error_kind: "provider".into(),
            completed_steps: vec!["step-0".into()],
            stuck_at: "step-1".into(),
            retry_parameters: Some(RetryParams {
                delay_ms: 30000,
                max_context_chars: None,
            }),
            suggested_action: RecoveryAction::Retry { delay_ms: 30000 },
        };
        let json = serde_json::to_string(&bundle).unwrap();
        let decoded: FailureBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.run_id, "r1");
        assert_eq!(
            decoded.suggested_action,
            RecoveryAction::Retry { delay_ms: 30000 }
        );
    }
}
