impl WriterAgentKernel {
    pub fn compute_cache_metrics(&self) -> super::WriterProductMetrics {
        let mut metrics = super::WriterProductMetrics::default();
        let events = self.run_events.recent(200);
        let mut spine_count = 0u64;
        let mut focus_rebuilds = 0u64;
        let mut miss_counts: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        let mut total_cached: u64 = 0;
        let mut total_input: u64 = 0;
        let mut spine_fingerprints: Vec<u64> = Vec::new();

        for event in &events {
            match event.event_type.as_str() {
                "context_spine" => {
                    spine_count += 1;
                    if let Some(fp) =
                        event.data.get("frozenFingerprint").and_then(|v| v.as_u64())
                    {
                        if let Some(&prev) = spine_fingerprints.last() {
                            if prev != fp {
                                focus_rebuilds += 1;
                            }
                        }
                        spine_fingerprints.push(fp);
                    }
                    if let Some(reasons) =
                        event.data.get("missReasons").and_then(|v| v.as_array())
                    {
                        for reason in reasons {
                            if let Some(r) = reason.as_str() {
                                let key = if r.contains("First call") {
                                    "first_call"
                                } else if r.contains("FrozenPrefix") {
                                    "frozen_prefix_changed"
                                } else if r.contains("ProjectStablePrefix") {
                                    "stable_prefix_changed"
                                } else if r.contains("FocusPack") {
                                    "focus_pack_changed"
                                } else if r.contains("Dynamic tail") {
                                    "dynamic_tail_large"
                                } else {
                                    "other"
                                };
                                *miss_counts.entry(key.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
                "prompt_cache" => {
                    if let Some(c) = event.data.get("cachedTokens").and_then(|v| v.as_u64()) {
                        total_cached += c;
                    }
                    if let Some(i) = event.data.get("promptTokens").and_then(|v| v.as_u64()) {
                        total_input += i;
                    }
                }
                _ => {}
            }
        }

        metrics.focus_pack_rebuild_count = focus_rebuilds;
        metrics.cache_miss_reason_counts = miss_counts;
        if total_input > 0 {
            metrics.cache_hit_token_ratio = total_cached as f64 / total_input as f64;
        }
        if spine_count > 1 {
            let unique_fps: std::collections::HashSet<u64> =
                spine_fingerprints.iter().copied().collect();
            if unique_fps.len() > 1 {
                metrics.static_prefix_churn_rate =
                    (unique_fps.len() as f64 - 1.0) / (spine_count as f64).max(1.0);
            }
        }
        metrics.estimated_cost_saved = (total_cached as f64) * 0.000_001;
        metrics
    }
}
