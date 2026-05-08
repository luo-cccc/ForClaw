# Targeted Revision Design

## Summary

对接已有 `ChapterQualityReport`，实现一次定向修订——选 top 3 低分项，构造 revision prompt，调用 LLM 修，修完重新评分。不改保存链路。

## Decisions

| 决策 | 选择 |
|------|------|
| 修订策略 | 单次 provider call，修完重评但不二次修订 |
| 低分项选择 | `top_revision_targets` (已有的 3 项) |
| 修订保护 | 强项不碰、canon 不碰、字数 ±10% |

---

## Files

| 文件 | 操作 | 职责 |
|------|------|------|
| `agent-writer-backend/src/chapter_generation/craft_quality.rs` | 修改 | 新增 `build_revision_prompt()` |
| `config/llm-request-profiles.json` | 修改 | 新增 `chapter_targeted_revision` profile |
| `agent-writer-backend/src/chapter_generation/draft_and_save.in.rs` | 修改 | Pipeline 中调用 revision |

---

## `build_revision_prompt()`

```rust
pub fn build_revision_prompt(
    chapter_text: &str,
    quality_report: &ChapterQualityReport,
    max_targets: usize,
) -> String {
    let targets: Vec<&QualityMetricResult> = quality_report
        .metric_results
        .iter()
        .filter(|m| m.severity == IssueSeverity::Major || m.severity == IssueSeverity::Fatal)
        .take(max_targets)
        .collect();

    if targets.is_empty() {
        return String::new();
    }

    let strong_metrics: Vec<&str> = quality_report
        .metric_results
        .iter()
        .filter(|m| m.score >= 0.8)
        .map(|m| m.metric.as_str())
        .collect();

    let mut prompt = String::from(
        "你是专业中文小说修订者。只修复下面列出的问题，不改其他任何内容。\n\n",
    );

    prompt.push_str("## 需要修复的问题\n\n");
    for (i, target) in targets.iter().enumerate() {
        prompt.push_str(&format!(
            "{}. **{}** (score {:.1}): {}\n   Revision hint: {}\n\n",
            i + 1, target.metric, target.score, target.reason, target.revision_hint
        ));
    }

    if !strong_metrics.is_empty() {
        prompt.push_str("## 必须保留的强项\n\n");
        prompt.push_str(&format!("以下指标已达标，修订不能破坏：{}\n\n", strong_metrics.join("、")));
    }

    prompt.push_str("## 硬约束\n\n");
    prompt.push_str("- 只修改与上述问题直接相关的句子和段落\n");
    prompt.push_str("- 不重写全章、不改变情节走向、不引入新人物或新设定\n");
    prompt.push_str("- 修改后字数变化不超过 ±10%\n");
    prompt.push_str("- 保留原文中所有已通过的写作特征\n\n");

    prompt.push_str("## 待修订正文\n\n");
    prompt.push_str(chapter_text);

    prompt
}
```

## LLM Profile

`config/llm-request-profiles.json` 新增：

```json
"chapter_targeted_revision": {
    "temperature": 0.3,
    "maxTokens": 4096,
    "disableReasoning": true
}
```

## Pipeline 集成

在 `draft_and_save.in.rs` 中的章节生成流程里，draft 生成后：

```rust
// After draft completion, if quality report has issues, attempt revision
if let Some(ref quality_report) = quality_report_before {
    if !quality_report.fatal_issues.is_empty() || !quality_report.major_issues.is_empty() {
        let revision_prompt = build_revision_prompt(&draft_text, quality_report, 3);
        if !revision_prompt.is_empty() {
            // Call provider with revision profile
            let revision_result = call_provider_with_profile(
                &revision_prompt, "chapter_targeted_revision"
            ).await;
            if let Ok(revised_text) = revision_result {
                // Re-evaluate quality
                let quality_report_after = evaluate_chapter_quality(
                    &revised_text, chapter_title, &scene_plan,
                    &open_promise_keywords, min_chars, max_chars,
                );
                // Use revised text if quality improved
                if quality_report_after.overall_score > quality_report.overall_score {
                    final_text = revised_text;
                    final_quality = Some(quality_report_after);
                }
            }
        }
    }
}
```

## 验收

```powershell
cargo fmt --check && cargo check --workspace && cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core && cargo test -p agent-writer --lib && cargo test -p forge-agent-mcp
```

- `build_revision_prompt` 空 quality report 返回空字符串
- `build_revision_prompt` 包含 top 3 问题 + revision hints + 硬约束
- 低分项数量 < 3 时只修实际数量
- 修订 pipeline 只在 revision 确实提高质量分时替换正文
