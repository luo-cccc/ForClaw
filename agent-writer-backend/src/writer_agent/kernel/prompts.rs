//! Prompt rendering and tool filter helpers for WriterAgentKernel.
//! Extracted from kernel.rs.

use crate::writer_agent::context::{AgentTask, WritingContextPack};
use crate::writer_agent::kernel::{
    tool_filter_for_task, WriterAgentApprovalMode, WriterAgentKernel, WriterAgentLedgerSnapshot,
    WriterAgentRunRequest, WriterAgentTask,
};
use crate::writer_agent::promise_planner::{plan_promise_payoffs, render_promise_payoff_plan};
use agent_harness_core::ToolFilter;

pub fn tool_filter_for_run_request(
    task: AgentTask,
    approval_mode: &WriterAgentApprovalMode,
) -> ToolFilter {
    let mut filter = tool_filter_for_task(task);
    if matches!(approval_mode, WriterAgentApprovalMode::ApprovedWrites) {
        filter.include_requires_approval = true;
    }
    filter
}

pub fn render_context_pack_for_prompt(pack: &WritingContextPack) -> String {
    let budget = &pack.budget_report;
    let source_report = if budget.source_reports.is_empty() {
        "- no source budgets consumed".to_string()
    } else {
        budget
            .source_reports
            .iter()
            .map(|report| {
                format!(
                    "- {}: requested {}, provided {}, truncated {}, reason: {}{}",
                    report.source,
                    report.requested,
                    report.provided,
                    report.truncated,
                    report.reason,
                    report
                        .truncation_reason
                        .as_ref()
                        .map(|reason| format!(", truncation: {}", reason))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let rendered_sources = pack
        .sources
        .iter()
        .map(|source| {
            format!(
                "## {:?} (priority {}, {} chars{})\n{}",
                source.source,
                source.priority,
                source.char_count,
                if source.truncated { ", truncated" } else { "" },
                source.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        "# ContextPack Budget\n\
task: {:?}\n\
used/budget: {}/{}\n\
wasted: {}\n\
sources:\n{}\n\n\
# ContextPack Sources\n{}",
        pack.task, budget.used, budget.total_budget, budget.wasted, source_report, rendered_sources
    )
}

pub fn source_refs_from_context_pack(pack: &WritingContextPack) -> Vec<String> {
    pack.sources
        .iter()
        .map(|source| format!("{:?}", source.source))
        .collect()
}

pub(crate) fn render_run_system_prompt(
    request: &WriterAgentRunRequest,
    context_pack: &WritingContextPack,
    kernel: &WriterAgentKernel,
) -> String {
    match request.task {
        WriterAgentTask::ManualRequest => {
            render_manual_run_system_prompt(request, context_pack, &kernel.ledger_snapshot())
        }
        WriterAgentTask::InlineRewrite => render_inline_run_system_prompt(request, context_pack),
        WriterAgentTask::GhostWriting => render_ghost_run_system_prompt(request, context_pack),
        WriterAgentTask::ChapterGeneration => {
            render_chapter_generation_run_system_prompt(request, context_pack)
        }
        WriterAgentTask::PlanningReview => render_planning_review_run_system_prompt(
            request,
            context_pack,
            &kernel.ledger_snapshot(),
        ),
        WriterAgentTask::ContinuityDiagnostic
        | WriterAgentTask::CanonMaintenance
        | WriterAgentTask::ProposalEvaluation => {
            render_diagnostic_run_system_prompt(request, context_pack)
        }
    }
}

pub(crate) fn render_manual_run_system_prompt(
    request: &WriterAgentRunRequest,
    context_pack: &WritingContextPack,
    ledger: &WriterAgentLedgerSnapshot,
) -> String {
    format!(
        "你是 Forge 的中文长篇小说写作 Agent，是作家的第二大脑和并肩创作伙伴，不是普通聊天助手，也不是只会补全文字的写作工具。\n\
你的任务是理解作者当前意图，结合项目长期记忆、设定、伏笔、风格偏好和当前稿件，给出可执行、可直接用于创作推进的回答。\n\
如果信息不足，先说明缺口；如果涉及人物、设定、时间线或伏笔，必须优先尊重 WriterAgent ContextPack 与 Ledger，不要随意发明冲突设定。\n\
回答要具体、短而有用；需要写正文时直接给可用正文，需要分析时给明确判断和下一步。\n\
如果建议会改变正文、canon、promise、outline 或长期记忆，只能提出可审查建议，不要声称已经写入。\n\n\
{}\n\n\
# WriterAgent ContextPack\n{}\n\n\
# WriterAgent Ledgers\n{}\n\n\
# Current draft tail\n\"\"\"\n{}\n\"\"\"\n\n\
# Focused paragraph\n\"\"\"\n{}\n\"\"\"\n\n\
# Selected text\n\"\"\"\n{}\n\"\"\"\n\n\
可使用工具检索 lorebook、outline 和章节资料；在虚构新信息前先查设定。",
        request.frontend_state.memory_context,
        render_context_pack_for_prompt(context_pack),
        render_ledger_snapshot_for_prompt(ledger),
        request.frontend_state.truncated_context,
        request.frontend_state.paragraph,
        request.frontend_state.selected_text,
    )
}

pub(crate) fn render_inline_run_system_prompt(
    request: &WriterAgentRunRequest,
    context_pack: &WritingContextPack,
) -> String {
    format!(
        "你是 Forge 的 Cursor 式中文小说写作 Agent。你只为当前光标生成可执行的正文改写或插入文本，不聊天，不解释，不输出 Markdown，不输出 XML action 标签。必须尊重 ContextPack、设定、伏笔和光标后文。\n\n\
作者指令: {}\n\
章节: {}\n\
选中文本:\n{}\n\n\
ContextPack:\n{}",
        request.user_instruction,
        request
            .observation
            .chapter_title
            .as_deref()
            .unwrap_or("current chapter"),
        request.observation.selected_text(),
        render_context_pack_for_prompt(context_pack)
    )
}

pub(crate) fn render_ghost_run_system_prompt(
    request: &WriterAgentRunRequest,
    context_pack: &WritingContextPack,
) -> String {
    format!(
        "你是一个中文长篇小说写作 Agent，不是聊天助手。你只负责在光标处提供可直接插入正文的一小段续写。必须尊重已给出的设定、伏笔、风格偏好和光标后文。不要解释，不要 Markdown，不要重复光标前文。输出 1-2 句中文正文。\n\n\
章节: {}\n光标位置: {}\n当前段落:\n{}\n\nContextPack:\n{}",
        request
            .observation
            .chapter_title
            .as_deref()
            .unwrap_or("current chapter"),
        request
            .observation
            .cursor
            .as_ref()
            .map(|cursor| cursor.to)
            .unwrap_or(0),
        request.observation.paragraph,
        render_context_pack_for_prompt(context_pack)
    )
}

pub(crate) fn render_chapter_generation_run_system_prompt(
    request: &WriterAgentRunRequest,
    context_pack: &WritingContextPack,
) -> String {
    format!(
        "你是 Forge 的长篇小说章节生成 Agent。根据 Chapter Mission、Story Contract、Promise Ledger、Canon 和 Result Feedback 写当前章草稿。不要覆盖既有正文；如信息不足，先说明缺口和风险。\n\n\
作者指令: {}\n\
章节: {}\n\n\
ContextPack:\n{}",
        request.user_instruction,
        request
            .observation
            .chapter_title
            .as_deref()
            .unwrap_or("current chapter"),
        render_context_pack_for_prompt(context_pack)
    )
}

pub(crate) fn render_planning_review_run_system_prompt(
    request: &WriterAgentRunRequest,
    context_pack: &WritingContextPack,
    ledger: &WriterAgentLedgerSnapshot,
) -> String {
    format!(
        "你是 Forge 的小说 Planning / Review 只读 Agent。你的职责是帮助作者规划下一步和审查当前章节方向，不是代写正文，也不是修改项目状态。\n\
硬约束：只读；不得输出可直接粘贴进正文的长段草稿；不得声称已经写入正文、Canon、Promise、Outline、Chapter Mission、Story Contract 或长期记忆；不得提出 typed write operation。\n\
必须基于 Story Contract、Chapter Mission、Promise Ledger、Canon、Project Brain 和当前稿件证据给判断。如果证据不足，明确列出缺口。\n\
输出结构必须包含：\n\
1. Objective understanding\n\
2. Evidence\n\
3. Risks\n\
4. Candidate next actions\n\
5. Questions needing author confirmation\n\n\
作者指令: {}\n\
章节: {}\n\n\
ContextPack:\n{}\n\n\
Ledgers:\n{}",
        request.user_instruction,
        request
            .observation
            .chapter_title
            .as_deref()
            .unwrap_or("current chapter"),
        render_context_pack_for_prompt(context_pack),
        render_planning_ledger_snapshot_for_prompt(
            ledger,
            request
                .observation
                .chapter_title
                .as_deref()
                .unwrap_or("current chapter"),
            &request.observation.paragraph
        )
    )
}

pub(crate) fn render_diagnostic_run_system_prompt(
    request: &WriterAgentRunRequest,
    context_pack: &WritingContextPack,
) -> String {
    format!(
        "你是 Forge 的小说连续性和设定诊断 Agent。只基于提供的证据指出风险、缺口和可审查建议；不要自动改写正文或长期记忆。\n\n\
作者指令: {}\n\
章节: {}\n\n\
ContextPack:\n{}",
        request.user_instruction,
        request
            .observation
            .chapter_title
            .as_deref()
            .unwrap_or("current chapter"),
        render_context_pack_for_prompt(context_pack)
    )
}

pub(crate) fn render_ledger_snapshot_for_prompt(snapshot: &WriterAgentLedgerSnapshot) -> String {
    let book_state = snapshot
        .book_state
        .as_ref()
        .map(|state| {
            let mut lines = Vec::new();
            if !state.long_term_constraints.is_empty() {
                lines.push(format!(
                    "- constraints: {}",
                    state.long_term_constraints.join(" / ")
                ));
            }
            if !state.mega_promises.is_empty() {
                lines.push(format!(
                    "- mega promises: {}",
                    state.mega_promises.join(" / ")
                ));
            }
            if !state.irreversible_changes.is_empty() {
                lines.push(format!(
                    "- irreversible changes: {}",
                    state.irreversible_changes.join(" / ")
                ));
            }
            lines.join("\n")
        })
        .unwrap_or_default();
    let volume = snapshot
        .active_volume
        .as_ref()
        .map(|volume| {
            format!(
                "- {} [{}-{}] status={}",
                volume.title, volume.start_chapter, volume.end_chapter, volume.status
            )
        })
        .unwrap_or_default();
    let arc = snapshot
        .arc_snapshots
        .iter()
        .take(2)
        .map(|arc| {
            format!(
                "- {} [{}-{}]",
                arc.title, arc.start_chapter, arc.end_chapter
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let canon = snapshot
        .canon_entities
        .iter()
        .take(8)
        .map(|entity| {
            format!(
                "- {} [{}]: {} {}",
                entity.name, entity.kind, entity.summary, entity.attributes
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let promises = snapshot
        .open_promises
        .iter()
        .take(8)
        .map(|promise| {
            format!(
                "- {} [{}]: {} -> {}",
                promise.title, promise.kind, promise.description, promise.expected_payoff
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let decisions = snapshot
        .recent_decisions
        .iter()
        .take(8)
        .map(|decision| {
            format!(
                "- {} / {}: {} ({})",
                decision.scope, decision.title, decision.decision, decision.rationale
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    [
        ("Book state", book_state),
        ("Active volume", volume),
        ("Arc snapshots", arc),
        ("Canon entities", canon),
        ("Open promises", promises),
        ("Recent creative decisions", decisions),
    ]
    .into_iter()
    .filter_map(|(label, content)| {
        if content.trim().is_empty() {
            None
        } else {
            Some(format!("## {}\n{}", label, content))
        }
    })
    .collect::<Vec<_>>()
    .join("\n\n")
}

pub(crate) fn render_planning_ledger_snapshot_for_prompt(
    snapshot: &WriterAgentLedgerSnapshot,
    current_chapter: &str,
    local_context: &str,
) -> String {
    let base = render_ledger_snapshot_for_prompt(snapshot);
    let payoff_plan = render_promise_payoff_plan(&plan_promise_payoffs(
        current_chapter,
        snapshot.active_chapter_mission.as_ref(),
        &snapshot.open_promises,
        local_context,
    ));
    if payoff_plan.trim().is_empty() {
        return base;
    }
    [base, format!("## Promise Payoff Plan\n{}", payoff_plan)]
        .into_iter()
        .filter(|section| !section.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn objective_for_run_task(task: &WriterAgentTask) -> String {
    match task {
        WriterAgentTask::ManualRequest => {
            "Answer the author's explicit request as a writing partner using current manuscript context and ledgers.".to_string()
        }
        WriterAgentTask::InlineRewrite => {
            "Produce a constrained inline rewrite or insertion that can be reviewed as a typed operation.".to_string()
        }
        WriterAgentTask::GhostWriting => {
            "Continue from the current cursor while preserving chapter mission, canon, and open promises.".to_string()
        }
        WriterAgentTask::ChapterGeneration => {
            "Generate chapter prose grounded in story contract, mission, promises, canon, and prior result feedback.".to_string()
        }
        WriterAgentTask::PlanningReview => {
            "Plan and review chapter direction using Story Contract, Chapter Mission, Promise Ledger, Canon, Project Brain, and current manuscript evidence without mutating project state.".to_string()
        }
        WriterAgentTask::ContinuityDiagnostic => {
            "Diagnose continuity, canon, mission, and promise risks using explicit evidence.".to_string()
        }
        WriterAgentTask::CanonMaintenance => {
            "Maintain canon candidates and conflicts without writing durable memory unless approved.".to_string()
        }
        WriterAgentTask::ProposalEvaluation => {
            "Evaluate a surfaced proposal against current evidence and feedback history.".to_string()
        }
    }
}

pub(crate) fn success_criteria_for_run_task(task: &WriterAgentTask) -> Vec<String> {
    match task {
        WriterAgentTask::ManualRequest => vec![
            "Answer directly addresses the author's request.".to_string(),
            "Canon, promise, mission, and story contract constraints are respected.".to_string(),
            "Any state-changing suggestion remains reviewable instead of being applied implicitly."
                .to_string(),
        ],
        WriterAgentTask::InlineRewrite => vec![
            "Output is limited to the selected range or cursor insertion point.".to_string(),
            "The rewrite remains compatible with context after the cursor.".to_string(),
        ],
        WriterAgentTask::GhostWriting => vec![
            "Continuation fits the local paragraph without forcing a broad rewrite.".to_string(),
            "Continuation does not introduce canon or promise-ledger conflicts.".to_string(),
        ],
        WriterAgentTask::ChapterGeneration => vec![
            "Chapter draft follows the active chapter mission.".to_string(),
            "Draft records risks around weak story contract or unresolved promises.".to_string(),
            "No durable write is implied without explicit approval and save.".to_string(),
        ],
        WriterAgentTask::PlanningReview => vec![
            "Plan cites evidence from Story Contract, Chapter Mission, Promise Ledger, Canon, Project Brain, or current manuscript context.".to_string(),
            "Output stays read-only and does not draft manuscript prose or imply project memory writes.".to_string(),
            "Candidate actions and author-confirmation questions are concrete enough to guide the next writing step.".to_string(),
        ],
        WriterAgentTask::ContinuityDiagnostic
        | WriterAgentTask::CanonMaintenance
        | WriterAgentTask::ProposalEvaluation => vec![
            "Findings cite evidence from context or ledgers.".to_string(),
            "Recommendations are reviewable and do not mutate project state directly.".to_string(),
        ],
    }
}
