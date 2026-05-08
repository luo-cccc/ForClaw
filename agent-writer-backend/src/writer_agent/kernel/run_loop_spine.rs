use super::*;

impl WriterAgentKernel {
    pub(crate) fn record_context_spine_run_events(
        &mut self,
        observation: &WriterObservation,
        context_pack: &WritingContextPack,
        ts_ms: u64,
    ) {
        let spine = crate::writer_agent::context::ContextSpine::from_pack(context_pack);
        let stability = spine.build_stability_report(self.last_spine.as_ref());
        self.run_events.append(
            &self.project_id,
            &self.session_id,
            "context_spine",
            ts_ms,
            Some(observation.id.clone()),
            vec![format!(
                "spine:{}:{}:{}",
                spine.frozen_fingerprint, spine.stable_fingerprint, spine.focus_fingerprint
            )],
            serde_json::json!({
                "frozenFingerprint": spine.frozen_fingerprint,
                "stableFingerprint": spine.stable_fingerprint,
                "focusFingerprint": spine.focus_fingerprint,
                "estimatedPrefixTokens": spine.estimated_prefix_tokens(),
                "estimatedDynamicTokens": spine.estimated_dynamic_tokens(),
                "missReasons": stability.miss_reasons,
                "prefixChurnSources": stability.prefix_churn_sources,
            }),
        );
        self.last_spine = Some(spine);

        let previous_node_id = self.focus.active_node_id.clone();
        let focus_changed = self.focus.switch_to(
            crate::writer_agent::context::FocusNodeKind::Chapter,
            observation.chapter_title.as_deref().unwrap_or("unknown"),
        );
        if focus_changed {
            self.run_events.append(
                &self.project_id,
                &self.session_id,
                "compaction_trigger",
                ts_ms,
                self.active_chapter.clone(),
                vec!["focus_node_switch".to_string()],
                serde_json::json!({
                    "trigger": "focus_node_switch",
                    "isDomainEvent": true,
                    "previousNodeId": previous_node_id,
                    "currentNodeId": self.focus.active_node_id,
                }),
            );
        }
    }

    pub(crate) fn record_compaction_trigger_event(
        &mut self,
        trigger: &agent_harness_core::CompactionTrigger,
        source_refs: &[String],
        ts_ms: u64,
    ) {
        self.run_events.append(
            &self.project_id,
            &self.session_id,
            "compaction_trigger",
            ts_ms,
            self.active_chapter.clone(),
            source_refs.to_vec(),
            serde_json::json!({
                "trigger": trigger.label(),
                "isDomainEvent": trigger.is_domain_event(),
            }),
        );
    }
}
