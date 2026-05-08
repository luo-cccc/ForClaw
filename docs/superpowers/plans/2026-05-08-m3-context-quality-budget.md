# M3: Context Quality + Budget Calibration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add ContextQualityReport for preflight output and BudgetCalibration for rolling token-per-char estimation — both in agent-harness-core.

**Architecture:** Two self-contained new modules. ContextQualityReport reads PackedContext to produce 4-dimension quality scores. BudgetCalibration uses `LazyLock<Mutex<CalibrationStore>>` for thread-safe global calibration with JSON persistence. AgentLoop integration minimal — one line in Complete event.

**Tech Stack:** Rust, serde, std::sync::LazyLock, std::sync::Mutex

---

## File Map

| File | Create/Modify | Responsibility |
|------|--------------|----------------|
| `agent-harness-core/src/context_quality.rs` | Create | ContextQualityReport + evaluate_context_quality() |
| `agent-harness-core/src/budget_calibration.rs` | Create | BudgetCalibration + CalibrationStore + global singleton |
| `config/token-calibration.json` | Create | Empty JSON array seed |
| `agent-harness-core/src/lib.rs` | Modify | Register 2 new modules + re-exports |
| `agent-harness-core/src/agent_loop.rs` | Modify | record_usage() call in Complete event |
| `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs` | Modify | BuiltChapterContext +context_quality field |
| `agent-writer-backend/src/chapter_generation/context.in.rs` | Modify | Call evaluate_context_quality after pack |

---

### Task 1: ContextQualityReport Module

**Files:**
- Create: `agent-harness-core/src/context_quality.rs`
- Modify: `agent-harness-core/src/lib.rs`

- [ ] **Step 1: Create context_quality.rs**

```rust
use serde::{Deserialize, Serialize};
use crate::context_pack::PackedContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextQualityReport {
    pub request_id: String,
    pub overall_score: f32,
    pub source_coverage: f32,
    pub truncation_risk: f32,
    pub grounding_quality: f32,
    pub missing_evidence: Vec<String>,
    pub warnings: Vec<String>,
    pub recommendation: ContextQualityRecommendation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextQualityRecommendation {
    Sufficient,
    Supplement { sources: Vec<String> },
    Critical { reason: String },
}

pub fn evaluate_context_quality(
    request_id: &str,
    packed: &PackedContext,
    required_sources: &[String],
) -> ContextQualityReport {
    let present_types: std::collections::HashSet<&str> = packed
        .sources
        .iter()
        .map(|s| s.source_type.as_str())
        .collect();

    let source_coverage = if required_sources.is_empty() {
        1.0
    } else {
        let covered = required_sources
            .iter()
            .filter(|req| present_types.contains(req.as_str()))
            .count();
        covered as f32 / required_sources.len().max(1) as f32
    };

    let missing_evidence: Vec<String> = required_sources
        .iter()
        .filter(|req| !present_types.contains(req.as_str()))
        .cloned()
        .collect();

    let truncation_risk = if packed.sources.is_empty() {
        0.0
    } else {
        packed.sources.iter().filter(|s| s.truncated).count() as f32
            / packed.sources.len() as f32
    };

    let core_types = ["outline", "lorebook", "chapter", "project_brain"];
    let diverse_count = core_types
        .iter()
        .filter(|t| present_types.contains(*t))
        .count();
    let grounding_quality = diverse_count as f32 / core_types.len() as f32;

    let overall_score = source_coverage * 0.4
        + (1.0 - truncation_risk) * 0.35
        + grounding_quality * 0.25;

    let mut warnings = Vec::new();
    if truncation_risk > 0.3 {
        warnings.push(format!(
            "{} of {} sources truncated",
            packed.sources.iter().filter(|s| s.truncated).count(),
            packed.sources.len()
        ));
    }

    let recommendation = if overall_score < 0.4 {
        ContextQualityRecommendation::Critical {
            reason: format!(
                "Context quality critically low ({:.0}%). Missing: {}",
                overall_score * 100.0,
                missing_evidence.join(", ")
            ),
        }
    } else if !missing_evidence.is_empty() || truncation_risk > 0.3 {
        ContextQualityRecommendation::Supplement {
            sources: missing_evidence.clone(),
        }
    } else {
        ContextQualityRecommendation::Sufficient
    };

    ContextQualityReport {
        request_id: request_id.to_string(),
        overall_score,
        source_coverage,
        truncation_risk,
        grounding_quality,
        missing_evidence,
        warnings,
        recommendation,
    }
}

#[cfg(test)]
mod context_quality_tests {
    use super::*;
    use crate::context_pack::{ContextSourceReport, PackedContext, ContextBudgetReport};

    fn make_packed(types: &[&str], truncated_mask: &[bool]) -> PackedContext {
        PackedContext {
            text: "test".into(),
            sources: types
                .iter()
                .enumerate()
                .map(|(i, t)| ContextSourceReport {
                    source_type: t.to_string(),
                    id: format!("s{}", i),
                    label: t.to_string(),
                    original_chars: 100,
                    included_chars: 100,
                    truncated: truncated_mask.get(i).copied().unwrap_or(false),
                    score: None,
                })
                .collect(),
            budget: ContextBudgetReport {
                max_chars: 1000,
                included_chars: 100,
                source_count: types.len(),
                truncated_source_count: truncated_mask.iter().filter(|&&t| t).count(),
                warnings: vec![],
            },
        }
    }

    #[test]
    fn empty_input_returns_sufficient() {
        let packed = make_packed(&[], &[]);
        let report = evaluate_context_quality("r1", &packed, &[]);
        assert_eq!(report.recommendation, ContextQualityRecommendation::Sufficient);
        assert!(report.overall_score > 0.9);
    }

    #[test]
    fn missing_required_source_detected() {
        let packed = make_packed(&["outline"], &[false]);
        let report = evaluate_context_quality("r2", &packed, &["outline".into(), "lorebook".into()]);
        assert_eq!(report.source_coverage, 0.5);
        assert!(report.missing_evidence.contains(&"lorebook".to_string()));
    }

    #[test]
    fn truncation_risk_detected() {
        let packed = make_packed(&["outline", "lorebook"], &[true, false]);
        let report = evaluate_context_quality("r3", &packed, &[]);
        assert!(report.truncation_risk > 0.0);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn all_sources_present_all_clean() {
        let packed = make_packed(&["outline", "lorebook", "chapter", "project_brain"], &[false; 4]);
        let required: Vec<String> = ["outline", "lorebook", "chapter"]
            .iter().map(|s| s.to_string()).collect();
        let report = evaluate_context_quality("r4", &packed, &required);
        assert_eq!(report.source_coverage, 1.0);
        assert_eq!(report.grounding_quality, 1.0);
        assert!(report.missing_evidence.is_empty());
    }

    #[test]
    fn critical_when_score_below_threshold() {
        let packed = make_packed(&[], &[]);
        let report = evaluate_context_quality("r5", &packed, &["outline".into(), "lorebook".into(), "chapter".into()]);
        assert!(matches!(report.recommendation, ContextQualityRecommendation::Critical { .. }));
    }
}
```

- [ ] **Step 2: Register in lib.rs**

```rust
pub mod context_quality;
```

Re-export:
```rust
pub use context_quality::{
    evaluate_context_quality, ContextQualityRecommendation, ContextQualityReport,
};
```

- [ ] **Step 3: Verify**

```powershell
cargo test -p agent-harness-core context_quality
cargo clippy -p agent-harness-core --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add agent-harness-core/src/context_quality.rs agent-harness-core/src/lib.rs
git commit -m "feat: add ContextQualityReport for preflight diagnostics

4-dimension quality scoring: source coverage, truncation risk,
grounding quality, overall score. Recommendation: Sufficient /
Supplement / Critical.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 2: BudgetCalibration Module

**Files:**
- Create: `agent-harness-core/src/budget_calibration.rs`
- Create: `config/token-calibration.json`
- Modify: `agent-harness-core/src/lib.rs`

- [ ] **Step 1: Create token-calibration.json**

```json
[]
```

- [ ] **Step 2: Create budget_calibration.rs**

```rust
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetCalibration {
    pub model: String,
    pub tokens_per_char: f32,
    pub samples: u64,
    pub last_error_ratio: f32,
    #[serde(default)]
    pub rolling_error_ratios: Vec<f32>,
}

impl BudgetCalibration {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            tokens_per_char: 1.0 / 3.0,
            samples: 0,
            last_error_ratio: 1.0,
            rolling_error_ratios: Vec::new(),
        }
    }

    pub fn record(&mut self, actual_input_tokens: u64, total_chars: usize) {
        if total_chars == 0 {
            return;
        }
        let observed = actual_input_tokens as f32 / total_chars as f32;
        self.tokens_per_char = self.tokens_per_char * 0.9 + observed * 0.1;
        self.last_error_ratio = observed / self.tokens_per_char.max(0.001);
        self.rolling_error_ratios.push(self.last_error_ratio);
        if self.rolling_error_ratios.len() > 10 {
            self.rolling_error_ratios.remove(0);
        }
        self.samples += 1;
    }

    pub fn estimate_tokens(&self, chars: usize) -> u64 {
        (chars as f32 * self.tokens_per_char).ceil() as u64
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationStore {
    pub entries: Vec<BudgetCalibration>,
}

impl CalibrationStore {
    pub fn get_or_create(&mut self, model: &str) -> &mut BudgetCalibration {
        if let Some(pos) = self.entries.iter().position(|e| e.model == model) {
            &mut self.entries[pos]
        } else {
            self.entries.push(BudgetCalibration::new(model));
            self.entries.last_mut().unwrap()
        }
    }

    fn load() -> Self {
        serde_json::from_str(include_str!("../../config/token-calibration.json"))
            .unwrap_or_default()
    }
}

static CALIBRATION: std::sync::LazyLock<Mutex<CalibrationStore>> =
    std::sync::LazyLock::new(|| Mutex::new(CalibrationStore::load()));

pub fn record_usage(model: &str, actual_input_tokens: u64, total_chars: usize) {
    if let Ok(mut store) = CALIBRATION.lock() {
        store.get_or_create(model).record(actual_input_tokens, total_chars);
    }
}

pub fn estimate_tokens(model: &str, chars: usize) -> u64 {
    if let Ok(store) = CALIBRATION.lock() {
        if let Some(entry) = store.entries.iter().find(|e| e.model == model) {
            return entry.estimate_tokens(chars);
        }
    }
    (chars as f32 / 3.0).ceil() as u64
}

#[cfg(test)]
mod budget_calibration_tests {
    use super::*;

    #[test]
    fn new_calibration_defaults_to_one_third() {
        let cal = BudgetCalibration::new("test-model");
        assert!((cal.tokens_per_char - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn record_updates_tokens_per_char() {
        let mut cal = BudgetCalibration::new("test-model");
        let original = cal.tokens_per_char;
        // Simulate: 100 chars → 40 tokens (observed = 0.4)
        cal.record(40, 100);
        assert!(cal.tokens_per_char > original);
        assert_eq!(cal.samples, 1);
    }

    #[test]
    fn record_ignores_zero_chars() {
        let mut cal = BudgetCalibration::new("test-model");
        let original = cal.tokens_per_char;
        cal.record(100, 0);
        assert_eq!(cal.tokens_per_char, original);
    }

    #[test]
    fn store_get_or_create_returns_existing() {
        let mut store = CalibrationStore::default();
        let entry = store.get_or_create("gpt-4o");
        entry.record(40, 100);
        let entry2 = store.get_or_create("gpt-4o");
        assert_eq!(entry2.samples, 1);
    }

    #[test]
    fn estimate_tokens_unknown_model_falls_back() {
        let tokens = estimate_tokens("unknown", 300);
        assert!(tokens >= 90 && tokens <= 110); // ~100 for 300 chars
    }
}
```

- [ ] **Step 3: Register in lib.rs**

```rust
pub mod budget_calibration;
```

Re-export:
```rust
pub use budget_calibration::{estimate_tokens, record_usage, BudgetCalibration, CalibrationStore};
```

- [ ] **Step 4: Verify**

```powershell
cargo test -p agent-harness-core budget_calibration
cargo clippy -p agent-harness-core --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add agent-harness-core/src/budget_calibration.rs agent-harness-core/src/lib.rs config/token-calibration.json
git commit -m "feat: add BudgetCalibration with rolling token-per-char estimation

Global singleton store with LazyLock<Mutex>. record_usage() updates
tokens_per_char using 0.1 learning rate. estimate_tokens() falls
back to chars/3 default for unknown models.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 3: Integration

**Files:**
- Modify: `agent-harness-core/src/agent_loop.rs`
- Modify: `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs`
- Modify: `agent-writer-backend/src/chapter_generation/context.in.rs`

- [ ] **Step 1: Integrate budget calibration into AgentLoop**

In `agent_loop.rs`, find the `Complete` event emission (where `self.emit(AgentLoopEvent::Complete { ... })` is called). Before the emit, add:

```rust
        // Record budget calibration from observed usage
        if let Some(ref usage) = usage {
            let total_chars: usize = self
                .messages
                .iter()
                .map(|m| m.content.as_ref().map(|c| c.chars().count()).unwrap_or(0))
                .sum::<usize>()
                + self.config.system_prompt.chars().count();
            crate::budget_calibration::record_usage(
                &model,
                usage.input_tokens,
                total_chars,
            );
        }
```

**Note:** `model` is only available inside the `provider_call_guard` block. You need to capture it. Simplest approach: add a `last_model` field to `AgentLoop`, set it in the provider guard section, use it here. Or just use `self.provider.models().into_iter().next().unwrap_or("unknown")`. Accept the simple approach for MVP.

- [ ] **Step 2: Add context_quality field to BuiltChapterContext**

In `types_and_utils.in.rs`, add to `BuiltChapterContext`:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_quality: Option<agent_harness_core::context_quality::ContextQualityReport>,
```

- [ ] **Step 3: Call evaluate_context_quality in build_chapter_context**

In `context.in.rs`, after `ContextPacker::finish()`, add:

```rust
    // Evaluate context quality
    let required_source_types: Vec<String> = input
        .budget
        .required_sources
        .iter()
        .map(|s| s.source_type.clone())
        .collect();
    let context_quality = Some(agent_harness_core::context_quality::evaluate_context_quality(
        &input.request_id,
        &packed,
        &required_source_types,
    ));
    let mut built = BuiltChapterContext { ... };
    built.context_quality = context_quality;
```

**NOTE:** The `Budget` struct may not have `required_sources`. In that case, pass `&[]` for MVP. Read the actual struct first.

- [ ] **Step 4: Verify full workspace**

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp
```

- [ ] **Step 5: Commit**

```bash
git add agent-harness-core/src/agent_loop.rs agent-writer-backend/src/chapter_generation/types_and_utils.in.rs agent-writer-backend/src/chapter_generation/context.in.rs
git commit -m "feat: integrate context quality and budget calibration

AgentLoop records budget calibration from observed usage.
BuiltChapterContext carries ContextQualityReport from preflight.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task Summary

| Task | Files | New Lines | Est. Time |
|------|-------|-----------|-----------|
| 1. ContextQualityReport | context_quality.rs, lib.rs | ~160 | 20 min |
| 2. BudgetCalibration | budget_calibration.rs, lib.rs, token-calibration.json | ~150 | 20 min |
| 3. Integration | agent_loop.rs, types_and_utils.in.rs, context.in.rs | ~30 | 20 min |
| **Total** | **8 files** | **~340** | **~1 hr** |
