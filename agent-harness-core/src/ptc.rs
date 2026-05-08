/// Programmatic Tool Calling — execute multi-step tool chains in one inference turn.
/// The LLM writes a script; intermediate tool results never enter context — only stdout.
/// Ported from Hermes Agent `code_execution_tool.py`.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtcConfig {
    pub max_tool_calls: u32,
    pub max_output_chars: usize,
    pub timeout_secs: u32,
    /// Only these tools are available inside PTC (safety whitelist).
    pub allowed_tools: Vec<String>,
}

impl Default for PtcConfig {
    fn default() -> Self {
        Self {
            max_tool_calls: 50,
            max_output_chars: 50_000,
            timeout_secs: 300,
            allowed_tools: vec![
                "load_current_chapter".into(),
                "search_lorebook".into(),
                "query_project_brain".into(),
                "load_outline_node".into(),
                "read_user_drift_profile".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtcScript {
    pub code: String,
    pub expected_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtcResult {
    pub stdout: String,
    pub tool_calls_made: u32,
    pub success: bool,
    pub error: Option<String>,
}

/// Build the PTC prompt that instructs the LLM to write a tool-calling script.
pub fn build_ptc_prompt(user_request: &str, available_tools: &[String]) -> String {
    format!(
        "你需要执行一个复杂的分析任务。你有权调用以下工具，但中间结果不会进入上下文。\n\n\
         ## 可用工具\n{}\n\n\
         ## 任务\n{}\n\n\
         ## 指令\n\
         编写一个Python脚本，使用 `hermes.call_tool(tool_name, **kwargs)` 调用工具。\n\
         - 每个工具调用返回一个字典，包含结果。\n\
         - 脚本的最后一行应该 `print()` 最终分析结果。\n\
         - 只输出Python代码，不要解释。代码放在 ```python ``` 块中。",
        available_tools
            .iter()
            .map(|t| format!("- {}", t))
            .collect::<Vec<_>>()
            .join("\n"),
        user_request,
    )
}

pub fn parse_ptc_output(stdout: &str) -> String {
    if stdout.chars().count() > 50_000 {
        let truncated: String = stdout.chars().take(50_000).collect();
        return format!(
            "{}\n\n[输出已截断，原始长度 {} 字符]",
            truncated,
            stdout.chars().count()
        );
    }
    stdout.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ptc_config_defaults() {
        let c = PtcConfig::default();
        assert_eq!(c.max_tool_calls, 50);
        assert!(c.allowed_tools.contains(&"search_lorebook".into()));
    }

    #[test]
    fn test_ptc_prompt_includes_request() {
        let p = build_ptc_prompt(
            "检查所有章节中'破庙'的出现是否一致",
            &["search_lorebook".into()],
        );
        assert!(p.contains("破庙"));
        assert!(p.contains("hermes.call_tool"));
    }

    #[test]
    fn test_ptc_output_truncation() {
        let long = "x".repeat(60_000);
        let r = parse_ptc_output(&long);
        assert!(r.chars().count() < 51_000);
        assert!(r.contains("截断"));
    }
}
