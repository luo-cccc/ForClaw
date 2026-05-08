#[cfg(test)]
mod tests {
    use super::*;

    fn outline() -> Vec<storage::OutlineNode> {
        vec![
            storage::OutlineNode {
                chapter_title: "第一章".to_string(),
                summary: "林墨抵达破庙。".to_string(),
                status: "drafted".to_string(),
            },
            storage::OutlineNode {
                chapter_title: "第二章".to_string(),
                summary: "林墨发现壁画。".to_string(),
                status: "drafted".to_string(),
            },
            storage::OutlineNode {
                chapter_title: "第三章".to_string(),
                summary: "林墨发现密道并遭遇毒雾机关。".to_string(),
                status: "empty".to_string(),
            },
        ]
    }

    #[test]
    fn counts_unicode_chars_instead_of_bytes_for_chinese_text() {
        assert_eq!(char_count("破庙密道"), 4);
        assert_eq!("破庙密道".len(), 12);
    }

    #[test]
    fn truncates_chinese_at_valid_utf8_boundary() {
        let (text, included, truncated) = truncate_text_report("林墨推开破庙石门", 4);
        assert_eq!(text, "林墨推开");
        assert_eq!(included, 4);
        assert!(truncated);
    }

    #[test]
    fn prefers_chinese_sentence_boundary_when_truncating() {
        let (text, _, truncated) =
            truncate_text_report("林墨停下脚步。毒雾从密道深处涌来，像潮水一样。", 16);
        assert_eq!(text, "林墨停下脚步。");
        assert!(truncated);
    }

    #[test]
    fn handles_mixed_chinese_english_and_emoji_without_corruption() {
        let (text, included, truncated) = truncate_text_report("AI提醒林墨：run！🔥继续。", 10);
        assert_eq!(char_count(&text), included);
        assert!(text.is_char_boundary(text.len()));
        assert!(truncated);
    }

    #[test]
    fn resolves_target_chapter_by_outline_number_and_returns_metadata() {
        let target = resolve_target_from_outline(&outline(), None, Some(3), None).unwrap();
        assert_eq!(target.title, "第三章");
        assert_eq!(target.number, Some(3));
        assert!(target.summary.contains("密道"));
    }

    #[test]
    fn rejects_missing_target_chapter_with_typed_error() {
        let err = resolve_target_from_outline(&outline(), Some("第九章"), None, None).unwrap_err();
        assert_eq!(err.code, "TARGET_CHAPTER_NOT_FOUND");
    }

    #[test]
    fn rejects_ambiguous_target_chapter_with_typed_error() {
        let mut data = outline();
        data.push(storage::OutlineNode {
            chapter_title: "第三章".to_string(),
            summary: "重复节点".to_string(),
            status: "empty".to_string(),
        });
        let err = resolve_target_from_outline(&data, Some("第三章"), None, None).unwrap_err();
        assert_eq!(err.code, "TARGET_CHAPTER_AMBIGUOUS");
    }

    #[test]
    fn replaces_chapter_when_revision_matches_and_frontend_is_clean() {
        let decision = decide_save_action(
            "第三章",
            "req-1",
            SaveMode::ReplaceIfClean,
            "abc",
            "abc",
            Some(&FrontendChapterStateSnapshot {
                open_chapter_title: Some("第三章".to_string()),
                open_chapter_revision: Some("abc".to_string()),
                dirty: false,
            }),
        );
        assert!(matches!(decision, SaveDecision::WriteTarget));
    }

    #[test]
    fn rejects_dirty_open_target_chapter_without_writing() {
        let decision = decide_save_action(
            "第三章",
            "req-1",
            SaveMode::ReplaceIfClean,
            "abc",
            "abc",
            Some(&FrontendChapterStateSnapshot {
                open_chapter_title: Some("第三章".to_string()),
                open_chapter_revision: Some("abc".to_string()),
                dirty: true,
            }),
        );
        match decision {
            SaveDecision::Conflict(conflict) => {
                assert_eq!(conflict.reason, "frontend_dirty_open_chapter");
            }
            _ => panic!("expected conflict"),
        }
    }

    #[test]
    fn rejects_revision_mismatch_without_writing() {
        let decision = decide_save_action(
            "第三章",
            "req-1",
            SaveMode::ReplaceIfClean,
            "abc",
            "def",
            None,
        );
        match decision {
            SaveDecision::Conflict(conflict) => {
                assert_eq!(conflict.reason, "revision_mismatch");
            }
            _ => panic!("expected conflict"),
        }
    }

    #[test]
    fn saves_draft_copy_on_conflict_only_when_requested() {
        let decision = decide_save_action(
            "第三章",
            "request-abcdef",
            SaveMode::SaveAsDraft,
            "abc",
            "def",
            None,
        );
        match decision {
            SaveDecision::WriteDraft {
                draft_title,
                conflict,
            } => {
                assert!(draft_title.contains("第三章 draft"));
                assert_eq!(conflict.reason, "revision_mismatch");
            }
            _ => panic!("expected draft decision"),
        }
    }

    #[test]
    fn rejects_empty_generated_content_with_content_empty() {
        let err = validate_generated_content(
            "  ",
            &ChapterContract::default(),
            ChapterContractPhase::ModelOutput,
        )
        .unwrap_err();
        assert_eq!(err.code, "MODEL_OUTPUT_EMPTY");
    }

    #[test]
    fn chapter_contract_rejects_inconsistent_bounds() {
        let err = ChapterContract {
            target_chars: 3_500,
            min_chars: 3_600,
            max_chars: 4_000,
            save_hard_floor_chars: 2_800,
            save_hard_ceiling_chars: 4_300,
        }
        .validate()
        .unwrap_err();
        assert_eq!(err.code, "CHAPTER_CONTRACT_INVALID");
    }

    #[test]
    fn rejects_model_output_below_contract_min_chars() {
        let err = validate_generated_content(
            &"甲".repeat(2_999),
            &ChapterContract::default(),
            ChapterContractPhase::ModelOutput,
        )
        .unwrap_err();
        assert_eq!(err.code, "MODEL_OUTPUT_UNDER_MIN_CHARS");
    }

    #[test]
    fn rejects_model_output_above_contract_max_chars() {
        let err = validate_generated_content(
            &"甲".repeat(4_001),
            &ChapterContract::default(),
            ChapterContractPhase::ModelOutput,
        )
        .unwrap_err();
        assert_eq!(err.code, "MODEL_OUTPUT_OVER_MAX_CHARS");
    }

    #[test]
    fn basic_output_validation_allows_repairable_length_variance() {
        validate_generated_content_basics(&"甲".repeat(2_999)).unwrap();
        validate_generated_content_basics(&"甲".repeat(4_001)).unwrap();
    }

    #[test]
    fn rejects_save_content_below_save_floor() {
        let err = validate_generated_content(
            &"甲".repeat(2_799),
            &ChapterContract::default(),
            ChapterContractPhase::Save,
        )
        .unwrap_err();
        assert_eq!(err.code, "CONTENT_UNDER_SAVE_FLOOR");
    }

    #[test]
    fn accepts_output_within_contract_bounds() {
        validate_generated_content(
            &"甲".repeat(3_500),
            &ChapterContract::default(),
            ChapterContractPhase::ModelOutput,
        )
        .unwrap();
        validate_generated_content(
            &"甲".repeat(3_500),
            &ChapterContract::default(),
            ChapterContractPhase::Save,
        )
        .unwrap();
    }

    #[test]
    fn chapter_contract_outcome_marks_under_min_and_over_max() {
        assert_eq!(
            chapter_contract_outcome(
                &"甲".repeat(2_999),
                &ChapterContract::default(),
                ChapterContractPhase::ModelOutput,
            ),
            ChapterContractOutcome::UnderMinChars
        );
        assert_eq!(
            chapter_contract_outcome(
                &"甲".repeat(4_001),
                &ChapterContract::default(),
                ChapterContractPhase::ModelOutput,
            ),
            ChapterContractOutcome::OverMaxChars
        );
    }

    #[test]
    fn maps_http_429_to_provider_rate_limited() {
        let err = map_provider_error("API error 429: too many requests".to_string());
        assert_eq!(err.code, "PROVIDER_RATE_LIMITED");
    }

    #[test]
    fn provider_budget_error_preserves_report_evidence() {
        let target = ChapterTarget {
            title: "第三章".to_string(),
            filename: "第三章.md".to_string(),
            number: Some(3),
            summary: "林墨发现密道。".to_string(),
            status: "empty".to_string(),
        };
        let receipt = build_chapter_generation_receipt(
            "budget-test-1",
            &target,
            "rev-1",
            "写第三章。",
            &[ChapterContextSource {
                source_type: "instruction".to_string(),
                id: "user-instruction".to_string(),
                label: "User instruction".to_string(),
                original_chars: 5,
                included_chars: 5,
                truncated: false,
                score: None,
            }],
            10,
        );
        let report = evaluate_provider_budget(WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ChapterGeneration,
            "gpt-4o",
            90_000,
            24_000,
        ));

        let error = provider_budget_error("budget-test-1", &receipt, report);

        assert_eq!(error.code, "PROVIDER_BUDGET_APPROVAL_REQUIRED");
        let evidence = error.evidence.expect("budget error has evidence");
        assert_eq!(evidence.category, WriterFailureCategory::ProviderFailed);
        assert!(evidence.details.get("providerBudget").is_some());
        assert!(!evidence.remediation.is_empty());
    }

    #[test]
    fn chapter_repair_budget_uses_repair_profile_output_tokens() {
        let settings = crate::llm_runtime::LlmSettings {
            api_key: "test".to_string(),
            api_base: "https://example.invalid".to_string(),
            model: "gpt-4o".to_string(),
            embedding_model: "text-embedding-3-small".to_string(),
            embedding_input_limit_chars: 8_000,
            chat_temperature: 0.7,
            json_temperature: 0.0,
            chat_max_tokens: 4_096,
            json_max_tokens: 1_024,
        };
        let messages = vec![serde_json::json!({"role": "user", "content": "续写"})];
        let continuation = chapter_generation_provider_budget_for_profile(
            &settings,
            &messages,
            crate::llm_runtime::LlmRequestProfile::ChapterContinuation,
        );
        let compress = chapter_generation_provider_budget_for_profile(
            &settings,
            &messages,
            crate::llm_runtime::LlmRequestProfile::ChapterCompress,
        );

        assert_eq!(continuation.requested_output_tokens, 2_200);
        assert_eq!(compress.requested_output_tokens, 5_200);
    }
}
