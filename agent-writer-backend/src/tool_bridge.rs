use agent_harness_core::tool_executor::{
    ToolExecution, ToolExecutionAuditEvent, ToolExecutionAuditSink, ToolHandler,
};
use tauri::AppHandle;
use tauri::Manager;

/// Real tool handler that bridges agent-harness-core tools to the Tauri app's storage layer.
pub struct TauriToolBridge {
    pub app: AppHandle,
}

pub fn writer_tool_audit_sink(
    app: AppHandle,
    task_id: impl Into<String>,
    source_refs: Vec<String>,
) -> ToolExecutionAuditSink {
    let task_id = task_id.into();
    std::sync::Arc::new(move |event| match event {
        ToolExecutionAuditEvent::Start { tool_name, input } => {
            record_writer_tool_called_start(&app, &task_id, tool_name, input, source_refs.clone());
        }
        ToolExecutionAuditEvent::End { execution } => {
            record_writer_tool_called_end(&app, &task_id, execution, source_refs.clone());
        }
    })
}

fn record_writer_tool_called_start(
    app: &AppHandle,
    task_id: &str,
    tool_name: String,
    input: serde_json::Value,
    mut source_refs: Vec<String>,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    source_refs.push(format!("tool:{}", tool_name));
    kernel.record_tool_called_run_event(
        task_id.to_string(),
        tool_name,
        "start",
        Some(&input),
        None,
        source_refs,
        crate::agent_runtime::now_ms(),
    );
}

fn record_writer_tool_called_end(
    app: &AppHandle,
    task_id: &str,
    execution: ToolExecution,
    mut source_refs: Vec<String>,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    source_refs.push(format!("tool:{}", execution.tool_name));
    kernel.record_tool_called_run_event(
        task_id.to_string(),
        execution.tool_name.clone(),
        "end",
        Some(&execution.input),
        Some(&execution),
        source_refs,
        crate::agent_runtime::now_ms(),
    );
}

#[async_trait::async_trait]
impl ToolHandler for TauriToolBridge {
    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match tool_name {
            "load_current_chapter" => {
                let chapter = string_arg(&args, &["chapter", "chapter_title"]);
                let content = crate::storage::load_chapter(&self.app, chapter.to_string())
                    .map_err(|e| format!("load_current_chapter: {}", e))?;
                Ok(serde_json::json!({"content": content, "chapter": chapter}))
            }
            "search_lorebook" => {
                let keyword = string_arg(&args, &["keyword", "keywords", "query"]);
                let entries = crate::storage::load_lorebook(&self.app)
                    .map_err(|e| format!("search_lorebook: {}", e))?;
                let matches: Vec<serde_json::Value> = entries
                    .iter()
                    .filter(|e| {
                        e.keyword.to_lowercase().contains(&keyword.to_lowercase())
                            || keyword.to_lowercase().contains(&e.keyword.to_lowercase())
                    })
                    .map(|e| serde_json::json!({"keyword": e.keyword, "content": e.content}))
                    .collect();
                Ok(serde_json::json!({"matches": matches}))
            }
            "load_outline_node" => {
                let id = string_arg(&args, &["chapter", "id", "chapter_title"]);
                let nodes = crate::storage::load_outline(&self.app)
                    .map_err(|e| format!("load_outline_node: {}", e))?;
                let node = nodes
                    .iter()
                    .find(|n| n.chapter_title == id)
                    .map(|n| {
                        serde_json::json!({
                            "chapter_title": n.chapter_title,
                            "summary": n.summary,
                            "status": n.status,
                        })
                    })
                    .unwrap_or(serde_json::json!({"error": "not found"}));
                Ok(node)
            }
            "query_project_brain" => {
                let query = string_arg(&args, &["query", "semantic_query"]);
                let api_key =
                    crate::resolve_api_key().ok_or_else(|| "No API key configured".to_string())?;
                let settings = crate::llm_runtime::settings(api_key);
                let focus = {
                    let state = self.app.state::<crate::AppState>();
                    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
                    crate::brain_service::ProjectBrainFocus::from_kernel(query, &kernel)
                };
                let mut result_text = String::new();
                crate::brain_service::answer_query_with_focus(
                    &self.app,
                    &settings,
                    query,
                    &focus,
                    None,
                    |content| {
                        result_text.push_str(&content);
                        Ok(crate::llm_runtime::StreamControl::Continue)
                    },
                )
                .await
                .map_err(|e| format!("query_project_brain: {}", e))?;
                Ok(serde_json::json!({"answer": result_text}))
            }
            "generate_bounded_continuation" => {
                let prompt = string_arg(&args, &["prompt", "context"]);
                let api_key =
                    crate::resolve_api_key().ok_or_else(|| "No API key configured".to_string())?;
                let settings = crate::llm_runtime::settings(api_key);
                let messages = vec![serde_json::json!({"role": "user", "content": prompt})];
                let result = crate::llm_runtime::chat_text_profile(
                    &settings,
                    messages,
                    crate::llm_runtime::LlmRequestProfile::ToolContinuation,
                    120,
                )
                .await
                .map_err(|e| format!("generate_bounded_continuation: {}", e))?;
                Ok(serde_json::json!({"text": result}))
            }
            "generate_chapter_draft" => {
                Err("generate_chapter_draft requires explicit approval.".into())
            }
            // Lightweight tools — no real I/O needed
            "read_user_drift_profile"
            | "load_domain_profile"
            | "pack_agent_context"
            | "plan_chapter_task"
            | "classify_writing_intent"
            | "record_run_trace" => Ok(serde_json::json!({"status": "ok", "tool": tool_name})),
            _ => Err(format!("Unknown tool: {}", tool_name)),
        }
    }
}

fn string_arg<'a>(args: &'a serde_json::Value, names: &[&str]) -> &'a str {
    names
        .iter()
        .find_map(|name| args.get(*name).and_then(|value| value.as_str()))
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_arg_reads_primary_and_legacy_aliases() {
        let args = serde_json::json!({
            "chapter_title": "Chapter-1",
            "semantic_query": "玉佩 去向"
        });

        assert_eq!(
            string_arg(&args, &["chapter", "chapter_title"]),
            "Chapter-1"
        );
        assert_eq!(string_arg(&args, &["query", "semantic_query"]), "玉佩 去向");
        assert_eq!(string_arg(&args, &["missing"]), "");
    }
}
