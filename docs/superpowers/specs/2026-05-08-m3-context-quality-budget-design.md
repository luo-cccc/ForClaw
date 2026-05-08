# M3: Context Quality + Budget Calibration Design

## Summary

两个独立但互补的子系统：ContextQualityReport 让上下文组装可评分可追溯；BudgetCalibration 让 token 估算随实际使用自我修正。两者都在 agent-harness-core 内实现，不改动 agent-writer-backend。

## Decisions

| 决策 | 选择 |
|------|------|
| ContextQualityReport 消费方 | preflight 输出，不自动降级 |
| Budget Calibration 粒度 | 内存滚动窗口 + JSON 持久化 |
| 交付方式 | 合并为一个 M3 milestone |

---

## Section 1: ContextQualityReport

### 新文件：`agent-harness-core/src/context_quality.rs`

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
    let mut report = ContextQualityReport {
        request_id: request_id.to_string(),
        overall_score: 1.0,
        source_coverage: 1.0,
        truncation_risk: 0.0,
        grounding_quality: 1.0,
        missing_evidence: Vec::new(),
        warnings: Vec::new(),
        recommendation: ContextQualityRecommendation::Sufficient,
    };

    let present_types: std::collections::HashSet<&str> = packed
        .sources
        .iter()
        .map(|s| s.source_type.as_str())
        .collect();

    // 1. Source coverage: how many required sources are present
    if !required_sources.is_empty() {
        let covered = required_sources
            .iter()
            .filter(|req| present_types.contains(req.as_str()))
            .count();
        report.source_coverage = covered as f32 / required_sources.len().max(1) as f32;
        report.missing_evidence = required_sources
            .iter()
            .filter(|req| !present_types.contains(req.as_str()))
            .cloned()
            .collect();
    }

    // 2. Truncation risk: proportion of sources that were truncated
    if !packed.sources.is_empty() {
        let truncated = packed.sources.iter().filter(|s| s.truncated).count();
        report.truncation_risk = truncated as f32 / packed.sources.len() as f32;
        if report.truncation_risk > 0.3 {
            report.warnings.push(format!(
                "{} of {} sources truncated — context may be incomplete",
                truncated,
                packed.sources.len()
            ));
        }
    }

    // 3. Grounding quality: source type diversity
    let core_types = ["outline", "lorebook", "chapter", "project_brain"];
    let diverse_count = core_types
        .iter()
        .filter(|t| present_types.contains(*t))
        .count();
    report.grounding_quality = diverse_count as f32 / core_types.len() as f32;

    // 4. Overall score = weighted average
    report.overall_score = report.source_coverage * 0.4
        + (1.0 - report.truncation_risk) * 0.35
        + report.grounding_quality * 0.25;

    // 5. Recommendation
    if report.overall_score < 0.4 {
        report.recommendation = ContextQualityRecommendation::Critical {
            reason: format!(
                "Context quality critically low ({:.0}%). Missing: {}",
                report.overall_score * 100.0,
                report.missing_evidence.join(", ")
            ),
        };
    } else if !report.missing_evidence.is_empty() || report.truncation_risk > 0.3 {
        report.recommendation = ContextQualityRecommendation::Supplement {
            sources: report.missing_evidence.clone(),
        };
    }

    report
}
```

### 插入点

在 `agent-writer-backend/src/chapter_generation/context.in.rs` 的 `build_chapter_context()` 返回前调用，将 `ContextQualityReport` 写入 `BuiltChapterContext` 的新增字段。

### BuiltChapterContext 新增字段

```rust
// 在 types_and_utils.in.rs 的 BuiltChapterContext 中新增：
pub context_quality: Option<ContextQualityReport>,
```

---

## Section 2: Budget Calibration

### 新文件：`agent-harness-core/src/budget_calibration.rs`

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
            tokens_per_char: 1.0 / 3.0, // default: 3 chars per token
            samples: 0,
            last_error_ratio: 1.0,
            rolling_error_ratios: Vec::new(),
        }
    }

    /// Update calibration from observed usage.
    /// `actual_input_tokens` = provider-reported input_tokens.
    /// `total_chars` = estimated character count of the full prompt context.
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

    pub fn load() -> Self {
        serde_json::from_str(include_str!("../../config/token-calibration.json"))
            .unwrap_or_default()
    }
}

/// Global calibration store.
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
```

### 新配置文件：`config/token-calibration.json`

```json
[]
```

### AgentLoop 集成

在 `agent_loop.rs` 的 `Complete` 事件 emit 前，如果 `usage` 有值：

```rust
if let Some(ref usage) = usage {
    let total_chars = self.messages.iter()
        .map(|m| m.content.as_ref().map(|c| c.chars().count()).unwrap_or(0))
        .sum::<usize>()
        + self.config.system_prompt.chars().count();
    budget_calibration::record_usage(
        &model,
        usage.input_tokens,
        total_chars,
    );
}
```

---

## Files Summary

| 文件 | 操作 | 职责 |
|------|------|------|
| `agent-harness-core/src/context_quality.rs` | 创建 | ContextQualityReport + evaluate_context_quality() |
| `agent-harness-core/src/budget_calibration.rs` | 创建 | BudgetCalibration + CalibrationStore + record_usage() |
| `agent-harness-core/src/lib.rs` | 修改 | 注册 2 个新模块 + 重导出 |
| `agent-harness-core/src/agent_loop.rs` | 修改 | Complete 事件前调用 record_usage() |
| `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs` | 修改 | BuiltChapterContext 新增 context_quality 字段 |
| `agent-writer-backend/src/chapter_generation/context.in.rs` | 修改 | build_chapter_context 返回前调用 evaluate_context_quality |
| `config/token-calibration.json` | 创建 | 空 JSON 数组 |
| `agent-harness-core/src/context_pack.rs` | 修改（可选） | ContextPacker 增加 required_sources 追踪 |

## Acceptance Criteria

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp
```

- `evaluate_context_quality` 空输入返回 `Sufficient`，不 panic
- `evaluate_context_quality` 缺失 required source 时 `missing_evidence` 非空
- `BudgetCalibration::record` 更新 `tokens_per_char`
- `record_usage` / `estimate_tokens` 线程安全
- 所有已有测试不回退

## Out of Scope

- Compiler 自适应降级（消费 ContextQualityReport 做决策）
- token-calibration.json 自动写回（MVP 只读+内存更新）
