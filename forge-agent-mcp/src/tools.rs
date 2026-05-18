use crate::BACKEND_ACTIONS;
use serde_json::{json, Value};

pub(crate) fn tools() -> Vec<Value> {
    let mut tools = vec![
        tool(
            "forge_backend_call",
            "Forge Backend Dispatcher",
            "Dispatch any supported backend action by name with JSON params. This is the stable single-entrypoint surface for plugin schedulers.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": BACKEND_ACTIONS
                    },
                    "params": { "type": "object" }
                },
                "additionalProperties": false,
                "required": ["action"]
            }),
        ),
        tool(
            "forge_manifest",
            "Forge Manifest",
            "Return server, protocol, transport, capability, and supported backend action metadata.",
            empty_schema(),
        ),
        tool(
            "forge_project_manifest",
            "Project Manifest",
            "Return the active headless Forge project manifest.",
            empty_schema(),
        ),
        tool(
            "forge_project_paths",
            "Project Paths",
            "Return active data, project, chapters, and writer-memory paths.",
            empty_schema(),
        ),
        tool(
            "forge_agent_tools",
            "Agent Tool Descriptors",
            "Return the backend writer-agent tool descriptors registered in Forge.",
            empty_schema(),
        ),
        tool(
            "forge_effective_tool_inventory",
            "Effective Tool Inventory",
            "Return allowed, blocked, and model-callable tool inventory after policy filtering.",
            empty_schema(),
        ),
        tool(
            "forge_agent_kernel_status",
            "Agent Kernel Status",
            "Return low-level tool registry, domain, quality gate, and trace status.",
            empty_schema(),
        ),
        tool(
            "forge_agent_domain_profile",
            "Agent Domain Profile",
            "Return the writing domain profile used by the agent harness.",
            empty_schema(),
        ),
        tool(
            "forge_status",
            "Forge Status",
            "Return project paths, active sprint, and writer-kernel status.",
            empty_schema(),
        ),
        tool(
            "forge_list_chapters",
            "List Chapters",
            "List local Markdown chapters in the active Forge project.",
            empty_schema(),
        ),
        tool(
            "forge_create_chapter",
            "Create Chapter",
            "Create an empty chapter file if it does not already exist.",
            title_schema(),
        ),
        tool(
            "forge_load_chapter",
            "Load Chapter",
            "Load a chapter's Markdown content.",
            title_schema(),
        ),
        tool(
            "forge_save_chapter",
            "Save Chapter",
            "Atomically save chapter content, record a revision, and notify the writer kernel.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["title", "content"]
            }),
        ),
        tool(
            "forge_chapter_revision",
            "Chapter Revision",
            "Return the content revision hash for a chapter.",
            title_schema(),
        ),
        tool(
            "forge_rename_chapter_file",
            "Rename Chapter File",
            "Rename a chapter Markdown file by filename.",
            json!({
                "type": "object",
                "properties": {
                    "oldName": { "type": "string" },
                    "newName": { "type": "string" }
                },
                "required": ["oldName", "newName"]
            }),
        ),
        tool(
            "forge_lorebook",
            "Load Lorebook",
            "Load lorebook entries for the active project.",
            empty_schema(),
        ),
        tool(
            "forge_save_lore_entry",
            "Save Lore Entry",
            "Create or update a lorebook entry.",
            json!({
                "type": "object",
                "properties": {
                    "keyword": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["keyword", "content"]
            }),
        ),
        tool(
            "forge_delete_lore_entry",
            "Delete Lore Entry",
            "Delete a lorebook entry by id.",
            id_schema(),
        ),
        tool(
            "forge_outline",
            "Load Outline",
            "Load outline nodes for the active project.",
            empty_schema(),
        ),
        tool(
            "forge_save_outline_node",
            "Save Outline Node",
            "Create or update an outline node and seed chapter mission memory.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" },
                    "summary": { "type": "string" },
                    "status": { "type": "string" }
                },
                "required": ["chapterTitle", "summary"]
            }),
        ),
        tool(
            "forge_delete_outline_node",
            "Delete Outline Node",
            "Delete an outline node by chapter title.",
            chapter_title_schema(),
        ),
        tool(
            "forge_update_outline_status",
            "Update Outline Status",
            "Update an outline node status.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" },
                    "status": { "type": "string" }
                },
                "required": ["chapterTitle", "status"]
            }),
        ),
        tool(
            "forge_reorder_outline_nodes",
            "Reorder Outline Nodes",
            "Reorder outline nodes by chapter title list.",
            json!({
                "type": "object",
                "properties": {
                    "orderedTitles": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["orderedTitles"]
            }),
        ),
        tool(
            "forge_list_volumes",
            "List Volumes",
            "List volume summaries from writer memory.",
            empty_schema(),
        ),
        tool(
            "forge_save_volume",
            "Save Volume",
            "Create or update a volume summary. Pass the VolumeSummary JSON object.",
            free_object_schema(),
        ),
        tool(
            "forge_delete_volume",
            "Delete Volume",
            "Delete a volume by id.",
            volume_id_schema(),
        ),
        tool(
            "forge_get_volume_snapshot",
            "Get Volume Snapshot",
            "Return the latest snapshot for a volume.",
            volume_id_schema(),
        ),
        tool(
            "forge_save_volume_snapshot",
            "Save Volume Snapshot",
            "Save a volume snapshot. Pass the VolumeSnapshotSummary JSON object.",
            free_object_schema(),
        ),
        tool(
            "forge_get_book_state",
            "Get Book State",
            "Return the project-level book state.",
            empty_schema(),
        ),
        tool(
            "forge_save_book_state",
            "Save Book State",
            "Save the project-level book state. Pass the BookStateSummary JSON object.",
            free_object_schema(),
        ),
        tool(
            "forge_observe",
            "Observe Writer State",
            "Send a typed WriterObservation into the writer kernel and return generated proposals.",
            json!({
                "type": "object",
                "properties": {
                    "observation": { "type": "object" }
                },
                "required": ["observation"]
            }),
        ),
        tool(
            "forge_ask_agent",
            "Ask Writer Agent",
            "Run the full headless writer-agent loop with project memory, tools, budget checks, and returned events.",
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "context": { "type": "string" },
                    "paragraph": { "type": "string" },
                    "selectedText": { "type": "string" },
                    "chapterTitle": { "type": "string" },
                    "chapterRevision": { "type": "string" },
                    "cursorPosition": { "type": "integer", "minimum": 0 },
                    "dirty": { "type": "boolean" },
                    "inlineOperation": { "type": "boolean" },
                    "providerBudgetApproval": { "type": "object" }
                },
                "required": ["message"]
            }),
        ),
        tool(
            "forge_generate_chapter_autonomous",
            "Generate Chapter Autonomous",
            "Run Forge's full chapter-generation pipeline headlessly: build context, draft/repair length, save with revision checks, apply settlement, persist runtime artifacts, update memory, and return events.",
            json!({
                "type": "object",
                "properties": {
                    "requestId": { "type": "string" },
                    "targetChapterTitle": { "type": "string" },
                    "targetChapterNumber": { "type": "integer", "minimum": 1 },
                    "userInstruction": { "type": "string" },
                    "budget": { "type": "object" },
                    "frontendState": { "type": "object" },
                    "saveMode": {
                        "type": "string",
                        "enum": ["create_if_missing", "replace_if_clean", "save_as_draft"]
                    },
                    "chapterSummaryOverride": { "type": "string" },
                    "chapterContract": { "type": "object" },
                    "providerBudgetApproval": { "type": "object" },
                    "qualityMode": {
                        "type": "string",
                        "enum": ["fast", "balanced", "strict"],
                        "description": "Quality mode: fast (no revision), balanced (revision on fatal/major), strict (revision on fatal/major plus low scene_repetition/plot_progression/new_information_density/state_delta_coverage)"
                    }
                },
                "required": ["userInstruction"]
            }),
        ),
        tool(
            "forge_batch_generate_chapter",
            "Batch Generate Chapter",
            "Generate one chapter using the batch-generation defaults with a target title and summary.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" },
                    "summary": { "type": "string" },
                    "frontendState": { "type": "object" },
                    "qualityMode": {
                        "type": "string",
                        "enum": ["fast", "balanced", "strict"],
                        "description": "Quality mode: fast (no revision), balanced (revision on fatal/major), strict (revision on fatal/major plus low scene_repetition/plot_progression/new_information_density/state_delta_coverage)"
                    }
                },
                "required": ["chapterTitle", "summary"]
            }),
        ),
        tool(
            "forge_repair_chapter_state",
            "Repair Chapter State",
            "Rebuild settlement/runtime artifacts and memory state for an existing chapter without rewriting chapter text.",
            chapter_title_schema(),
        ),
        tool(
            "forge_analyze_chapter",
            "Analyze Chapter",
            "Use the configured model to return structured editorial review items for chapter content.",
            json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string" }
                },
                "required": ["content"]
            }),
        ),
        tool(
            "forge_generate_parallel_drafts",
            "Generate Parallel Drafts",
            "Generate three alternative continuation drafts for the current cursor context.",
            json!({
                "type": "object",
                "properties": {
                    "prefix": { "type": "string" },
                    "suffix": { "type": "string" },
                    "paragraph": { "type": "string" },
                    "selectedText": { "type": "string" },
                    "chapterTitle": { "type": "string" },
                    "missionContext": { "type": "string" },
                    "promiseContext": { "type": "string" }
                },
                "required": ["prefix", "suffix", "paragraph", "selectedText"]
            }),
        ),
        tool(
            "forge_analyze_pacing",
            "Analyze Pacing",
            "Use the configured model to analyze pacing from chapter summaries.",
            json!({
                "type": "object",
                "properties": {
                    "summaries": { "type": "string" }
                },
                "required": ["summaries"]
            }),
        ),
        tool(
            "forge_ask_project_brain",
            "Ask Project Brain",
            "Ask the model a question grounded in the local Project Brain vector index.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "providerBudgetApproval": { "type": "object" }
                },
                "required": ["query"]
            }),
        ),
        tool(
            "forge_project_brain_knowledge_graph",
            "Project Brain Knowledge Graph",
            "Return or rebuild the Project Brain knowledge graph index.",
            empty_schema(),
        ),
        tool(
            "forge_compare_project_brain_source_revisions",
            "Compare Project Brain Revisions",
            "Compare active and archived revisions for a Project Brain source reference.",
            json!({
                "type": "object",
                "properties": {
                    "sourceRef": { "type": "string" }
                },
                "required": ["sourceRef"]
            }),
        ),
        tool(
            "forge_restore_project_brain_source_revision",
            "Restore Project Brain Revision",
            "Restore a Project Brain source reference to a specific revision.",
            json!({
                "type": "object",
                "properties": {
                    "sourceRef": { "type": "string" },
                    "revision": { "type": "string" }
                },
                "required": ["sourceRef", "revision"]
            }),
        ),
        tool(
            "forge_cross_reference_brain_nodes",
            "Cross Reference Brain Nodes",
            "Create a suggested cross-reference between two Project Brain knowledge nodes.",
            json!({
                "type": "object",
                "properties": {
                    "sourceNodeId": { "type": "string" },
                    "targetNodeId": { "type": "string" }
                },
                "required": ["sourceNodeId", "targetNodeId"]
            }),
        ),
        tool(
            "forge_ingest_external_research",
            "Ingest External Research",
            "Author-approved ingest of an external research source into Project Brain.",
            json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string" },
                    "urlOrPath": { "type": "string" },
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "authorApproved": { "type": "boolean" },
                    "approvalReason": { "type": "string" }
                },
                "required": ["provider", "title", "content", "authorApproved", "approvalReason"]
            }),
        ),
        tool(
            "forge_run_metacognitive_recovery",
            "Run Metacognitive Recovery",
            "Run a read-only planning review or continuity diagnostic through the headless writer-agent loop.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["planning_review", "continuity_diagnostic"]
                    },
                    "instruction": { "type": "string" },
                    "context": { "type": "string" },
                    "paragraph": { "type": "string" },
                    "selectedText": { "type": "string" },
                    "chapterTitle": { "type": "string" },
                    "chapterRevision": { "type": "string" },
                    "cursorPosition": { "type": "integer", "minimum": 0 },
                    "dirty": { "type": "boolean" },
                    "providerBudgetApproval": { "type": "object" }
                },
                "required": ["action"]
            }),
        ),
        tool(
            "forge_ledger",
            "Writer Ledger",
            "Return story contract, missions, canon, promises, decisions, and memory reliability.",
            empty_schema(),
        ),
        tool(
            "forge_today_five",
            "Today Five",
            "Return the compact companion summary for the current project/chapter.",
            empty_schema(),
        ),
        tool(
            "forge_pending_proposals",
            "Pending Proposals",
            "Return currently pending writer-agent proposals.",
            empty_schema(),
        ),
        tool(
            "forge_story_review_queue",
            "Story Review Queue",
            "Return review queue entries derived from non-ghost proposals.",
            empty_schema(),
        ),
        tool(
            "forge_story_debt",
            "Story Debt",
            "Return unresolved story debt across contract, mission, canon, promise, and pacing categories.",
            empty_schema(),
        ),
        tool(
            "forge_reader_compensation_review_chain",
            "Reader Compensation Chain",
            "Return reader-compensation review chain diagnostics.",
            empty_schema(),
        ),
        tool(
            "forge_trace",
            "Writer Trace",
            "Return recent writer-agent trace events.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                }
            }),
        ),
        tool(
            "forge_inspector_timeline",
            "Inspector Timeline",
            "Return recent writer inspector timeline events.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                }
            }),
        ),
        tool(
            "forge_companion_timeline_summary",
            "Companion Timeline Summary",
            "Return the compact companion-facing timeline summary.",
            empty_schema(),
        ),
        tool(
            "forge_apply_feedback",
            "Apply Proposal Feedback",
            "Record accepted/rejected/edited/snoozed/explained proposal feedback.",
            json!({
                "type": "object",
                "properties": {
                    "proposalId": { "type": "string" },
                    "action": {
                        "type": "string",
                        "enum": ["accepted", "rejected", "edited", "snoozed", "explained"]
                    },
                    "finalText": { "type": "string" },
                    "reason": { "type": "string" },
                    "createdAt": { "type": "integer" }
                },
                "required": ["proposalId", "action", "createdAt"]
            }),
        ),
        tool(
            "forge_record_implicit_ghost_rejection",
            "Record Ghost Rejection",
            "Record an implicit rejection for an expired ghost proposal.",
            json!({
                "type": "object",
                "properties": {
                    "proposalId": { "type": "string" },
                    "createdAt": { "type": "integer" }
                },
                "required": ["proposalId", "createdAt"]
            }),
        ),
        tool(
            "forge_approve_writer_operation",
            "Approve Writer Operation",
            "Approve and execute a typed writer operation using explicit approval context.",
            json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "object" },
                    "currentRevision": { "type": "string" },
                    "approval": { "type": "object" }
                },
                "required": ["operation", "currentRevision"]
            }),
        ),
        tool(
            "forge_record_writer_operation_durable_save",
            "Record Durable Save",
            "Record durable save outcome for a writer operation.",
            free_object_schema(),
        ),
        tool(
            "forge_ambient_entity_hints",
            "Ambient Entity Hints",
            "Return canon entity hints for the given paragraph and chapter.",
            json!({
                "type": "object",
                "properties": {
                    "paragraph": { "type": "string" },
                    "chapter": { "type": "string" }
                },
                "required": ["paragraph", "chapter"]
            }),
        ),
        tool(
            "forge_start_sprint",
            "Start Supervised Sprint",
            "Create a supervised multi-chapter sprint plan.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitles": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1
                    },
                    "requireApprovalPerChapter": { "type": "boolean" },
                    "maxChaptersPerSession": { "type": "integer", "minimum": 1 },
                    "budgetCeilingMicros": { "type": "integer", "minimum": 0 }
                },
                "required": ["chapterTitles"]
            }),
        ),
        tool(
            "forge_sprint_plan",
            "Sprint Plan",
            "Return the active supervised sprint plan, if any.",
            empty_schema(),
        ),
        tool(
            "forge_sprint_progress",
            "Sprint Progress",
            "Return current supervised sprint progress, if one exists.",
            empty_schema(),
        ),
        tool(
            "forge_pause_sprint",
            "Pause Sprint",
            "Pause the active supervised sprint.",
            empty_schema(),
        ),
        tool(
            "forge_resume_sprint",
            "Resume Sprint",
            "Resume the active supervised sprint.",
            empty_schema(),
        ),
        tool(
            "forge_cancel_sprint",
            "Cancel Sprint",
            "Cancel the active supervised sprint.",
            empty_schema(),
        ),
        tool(
            "forge_checkpoint_sprint",
            "Checkpoint Sprint",
            "Record a checkpoint for the active supervised sprint.",
            json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string" }
                }
            }),
        ),
        tool(
            "forge_record_sprint_budget_usage",
            "Record Sprint Budget Usage",
            "Add provider spend to the active supervised sprint budget accounting.",
            json!({
                "type": "object",
                "properties": {
                    "spentMicros": { "type": "integer", "minimum": 0 }
                },
                "required": ["spentMicros"]
            }),
        ),
        tool(
            "forge_set_sprint_quality_gate",
            "Set Sprint Quality Gate",
            "Configure quality thresholds for the active supervised sprint. Blocks chapter advancement when quality drops below the minimum score or a fatal issue is detected.",
            json!({
                "type": "object",
                "properties": {
                    "minimumQualityScore": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                    "stopOnFatalIssue": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "forge_latest_chapter_generation_checkpoint",
            "Latest Chapter Generation Checkpoint",
            "Query the latest checkpoint for a chapter generation task.",
            json!({
                "type": "object",
                "properties": {
                    "taskId": { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "forge_chapter_generation_resume_candidates",
            "Chapter Generation Resume Candidates",
            "List recent chapter generation checkpoints that could be used to resume interrupted work.",
            empty_schema(),
        ),
        tool(
            "forge_resume_chapter_generation",
            "Resume Chapter Generation",
            "Resume a chapter generation from a saved checkpoint. Returns a structured resume plan with completed steps, recommended recovery step, and any missing context.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "checkpointId": { "type": "string" }
                },
                "required": ["checkpointId"],
                "additionalProperties": false
            }),
        ),
        tool(
            "forge_craft_library",
            "Craft Library",
            "Return the loaded craft rule library with all available writing quality rules.",
            empty_schema(),
        ),
        tool(
            "forge_external_writing_db_status",
            "External Writing DB Status",
            "Return configuration and readability status for the external writing-situation SQLite database.",
            empty_schema(),
        ),
        tool(
            "forge_external_writing_task_queries",
            "External Writing Task Queries",
            "List curated writing-problem entries from the external writing-situation SQLite database.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "forge_external_writing_search",
            "External Writing Search",
            "Search the external writing-situation SQLite database for task queries, rule mappings, workflow rules, and curated references.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        tool(
            "forge_craft_memory_stats",
            "Craft Memory Stats",
            "Return craft memory statistics: rule acceptance/rejection rates plus recent good examples and bad patterns across the project.",
            empty_schema(),
        ),
        tool(
            "forge_eval_trend_summary",
            "Eval Trend Summary",
            "Return the writing eval trend summary: per-profile pass/fail counts, metric averages, craft rule trends, and markdown report. Read-only.",
            empty_schema(),
        ),
        tool(
            "forge_record_manual_craft_edit_feedback",
            "Record Manual Craft Edit Feedback",
            "Record an author-approved before/after manuscript edit as craft memory, capturing improved examples and rejected bad patterns.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" },
                    "beforeText": { "type": "string" },
                    "afterText": { "type": "string" },
                    "metrics": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "anchorKeywords": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "openPromiseKeywords": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "targetMinChars": { "type": "integer", "minimum": 0 },
                    "targetMaxChars": { "type": "integer", "minimum": 1 },
                    "sourceRef": { "type": "string" },
                    "authorApproved": { "type": "boolean" }
                },
                "required": ["chapterTitle", "beforeText", "afterText", "authorApproved"],
                "additionalProperties": false
            }),
        ),
        tool(
            "forge_chapter_quality_report",
            "Chapter Quality Report",
            "Evaluate chapter text quality across 8 metrics and return a quality report with scores, issues, and revision targets.",
            json!({
                "type": "object",
                "properties": {
                    "chapterText": { "type": "string" },
                    "chapterTitle": { "type": "string" },
                    "targetMinChars": { "type": "integer", "minimum": 1 },
                    "targetMaxChars": { "type": "integer", "minimum": 1 }
                },
                "required": ["chapterText", "chapterTitle"]
            }),
        ),
        tool(
            "forge_context_quality_report",
            "Context Quality Report",
            "Evaluate context quality for a chapter generation request and return completeness and relevance diagnostics.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" }
                },
                "required": ["chapterTitle"]
            }),
        ),
        tool(
            "forge_budget_calibration",
            "Budget Calibration",
            "Return chapter context budget calibration diagnostics for the current project.",
            empty_schema(),
        ),
        tool(
            "forge_execution_plan",
            "Execution Plan",
            "Return the current chapter generation execution plan with strategy and budget details.",
            empty_schema(),
        ),
        tool(
            "forge_project_graph_data",
            "Project Graph Data",
            "Return headless project graph data from lore, canon, chapters, and outline.",
            empty_schema(),
        ),
        tool(
            "forge_project_storage_diagnostics",
            "Project Storage Diagnostics",
            "Return diagnostics for project JSON files, chapter files, and writer memory database.",
            empty_schema(),
        ),
        tool(
            "forge_export_writer_agent_trajectory",
            "Export Writer Trajectory",
            "Export writer-agent trajectory JSONL to the local data log directory.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000 },
                    "format": {
                        "type": "string",
                        "enum": ["jsonl", "trace_viewer", "claude_code", "hf_agent_trace_viewer"]
                    }
                }
            }),
        ),
        tool(
            "forge_export_diagnostic_logs",
            "Export Diagnostic Logs",
            "Export diagnostic logs and storage diagnostics to a local zip file.",
            empty_schema(),
        ),
        tool(
            "forge_list_file_backups",
            "List File Backups",
            "List backups for a target project file.",
            backup_target_schema(),
        ),
        tool(
            "forge_restore_file_backup",
            "Restore File Backup",
            "Restore a backup for a target project file.",
            json!({
                "type": "object",
                "properties": {
                    "target": backup_target_schema(),
                    "backupId": { "type": "string" }
                },
                "required": ["target", "backupId"]
            }),
        ),
        tool(
            "forge_set_api_key",
            "Set API Key",
            "Store an API key for a provider in the backend keyring.",
            json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string" },
                    "key": { "type": "string" }
                },
                "required": ["provider", "key"]
            }),
        ),
        tool(
            "forge_check_api_key",
            "Check API Key",
            "Check whether an API key is available for a provider.",
            json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string" }
                },
                "required": ["provider"]
            }),
        ),
        tool(
            "forge_list_world_assets",
            "List World Assets",
            "List world-building assets for the active project, including their approval status and evidence.",
            empty_schema(),
        ),
        tool(
            "forge_approve_world_asset",
            "Approve World Asset",
            "Approve a proposed world asset so it can be used as a hard constraint source.",
            json!({
                "type": "object",
                "properties": {
                    "assetId": { "type": "string" }
                },
                "required": ["assetId"]
            }),
        ),
        tool(
            "forge_reject_world_asset",
            "Reject World Asset",
            "Reject a proposed world asset.",
            json!({
                "type": "object",
                "properties": {
                    "assetId": { "type": "string" }
                },
                "required": ["assetId"]
            }),
        ),
        tool(
            "forge_world_bible_constraint_query",
            "World Bible Constraint Query",
            "Query a constraint by ID to retrieve its source reference, approval status, usage chapters, and conflicting rules.",
            json!({
                "type": "object",
                "properties": {
                    "constraintId": { "type": "string", "description": "The ID of the canon constraint to query" }
                },
                "required": ["constraintId"]
            }),
        ),
    ];
    tools.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or("")
            .cmp(right["name"].as_str().unwrap_or(""))
    });
    tools
}

fn tool(name: &str, title: &str, description: &str, input_schema: Value) -> Value {
    let read_only = is_read_only_tool(name);
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": tool_output_schema(),
        "annotations": {
            "title": title,
            "readOnlyHint": read_only,
            "destructiveHint": is_destructive_tool(name),
            "idempotentHint": is_idempotent_tool(name),
            "openWorldHint": is_open_world_tool(name)
        },
        "_meta": {
            "forge/action": name.strip_prefix("forge_").unwrap_or(name),
            "forge/backend": "headless",
            "forge/readOnly": read_only
        }
    })
}

fn tool_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "data": {},
            "error": {
                "anyOf": [
                    { "type": "null" },
                    {
                        "type": "object",
                        "properties": {
                            "message": { "type": "string" },
                            "kind": { "type": "string" }
                        },
                        "required": ["message", "kind"],
                        "additionalProperties": true
                    }
                ]
            }
        },
        "required": ["ok", "data", "error"],
        "additionalProperties": false
    })
}

pub(crate) fn is_read_only_tool(name: &str) -> bool {
    matches!(
        name,
        "forge_manifest"
            | "forge_project_manifest"
            | "forge_project_paths"
            | "forge_agent_tools"
            | "forge_effective_tool_inventory"
            | "forge_agent_kernel_status"
            | "forge_agent_domain_profile"
            | "forge_status"
            | "forge_list_chapters"
            | "forge_load_chapter"
            | "forge_chapter_revision"
            | "forge_lorebook"
            | "forge_outline"
            | "forge_list_volumes"
            | "forge_get_volume_snapshot"
            | "forge_get_book_state"
            | "forge_ledger"
            | "forge_today_five"
            | "forge_pending_proposals"
            | "forge_story_review_queue"
            | "forge_story_debt"
            | "forge_reader_compensation_review_chain"
            | "forge_trace"
            | "forge_inspector_timeline"
            | "forge_companion_timeline_summary"
            | "forge_analyze_chapter"
            | "forge_generate_parallel_drafts"
            | "forge_analyze_pacing"
            | "forge_ask_project_brain"
            | "forge_sprint_plan"
            | "forge_sprint_progress"
            | "forge_project_graph_data"
            | "forge_project_storage_diagnostics"
            | "forge_list_file_backups"
            | "forge_check_api_key"
            | "forge_world_bible_constraint_query"
    )
}

pub(crate) fn is_destructive_tool(name: &str) -> bool {
    matches!(
        name,
        "forge_backend_call"
            | "forge_delete_lore_entry"
            | "forge_delete_outline_node"
            | "forge_delete_volume"
            | "forge_restore_project_brain_source_revision"
            | "forge_cancel_sprint"
            | "forge_restore_file_backup"
            | "forge_rename_chapter_file"
    )
}

pub(crate) fn is_idempotent_tool(name: &str) -> bool {
    is_read_only_tool(name)
        || matches!(
            name,
            "forge_create_chapter"
                | "forge_save_chapter"
                | "forge_save_lore_entry"
                | "forge_save_outline_node"
                | "forge_update_outline_status"
                | "forge_reorder_outline_nodes"
                | "forge_save_volume"
                | "forge_save_volume_snapshot"
                | "forge_save_book_state"
                | "forge_record_writer_operation_durable_save"
                | "forge_pause_sprint"
                | "forge_resume_sprint"
                | "forge_set_api_key"
        )
}

pub(crate) fn is_open_world_tool(name: &str) -> bool {
    matches!(
        name,
        "forge_backend_call"
            | "forge_ask_agent"
            | "forge_batch_generate_chapter"
            | "forge_generate_chapter_autonomous"
            | "forge_analyze_chapter"
            | "forge_generate_parallel_drafts"
            | "forge_analyze_pacing"
            | "forge_ask_project_brain"
            | "forge_project_brain_knowledge_graph"
            | "forge_compare_project_brain_source_revisions"
            | "forge_restore_project_brain_source_revision"
            | "forge_cross_reference_brain_nodes"
            | "forge_ingest_external_research"
            | "forge_run_metacognitive_recovery"
    )
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false
    })
}

fn title_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" }
        },
        "required": ["title"]
    })
}

fn id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" }
        },
        "required": ["id"]
    })
}

fn chapter_title_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "chapterTitle": { "type": "string" }
        },
        "required": ["chapterTitle"]
    })
}

fn volume_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "volumeId": { "type": "string" }
        },
        "required": ["volumeId"]
    })
}

fn free_object_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true
    })
}

fn backup_target_schema() -> Value {
    json!({
        "type": "object",
        "oneOf": [
            {
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["lorebook", "outline", "project_brain"]
                    }
                },
                "required": ["kind"]
            },
            {
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["chapter"]
                    },
                    "title": { "type": "string" }
                },
                "required": ["kind", "title"]
            }
        ]
    })
}
