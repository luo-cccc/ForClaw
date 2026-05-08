use crate::writer_agent::context::WritingContextPack;
use crate::writer_agent::kernel::WriterContextPackBuiltSourceReport;
use crate::writer_agent::observation::WriterObservation;
use crate::writer_agent::proposal::AgentProposal;

pub(in crate::writer_agent::kernel::trace_recording) fn observation_source_refs(
    observation: &WriterObservation,
) -> Vec<String> {
    let mut refs = vec![observation.id.clone()];
    if let Some(chapter) = observation.chapter_title.as_ref() {
        refs.push(format!("chapter:{}", chapter));
    }
    if let Some(revision) = observation.chapter_revision.as_ref() {
        refs.push(format!("revision:{}", revision));
    }
    refs
}

pub(in crate::writer_agent::kernel::trace_recording) fn proposal_source_refs(
    proposal: &AgentProposal,
) -> Vec<String> {
    let mut refs = vec![proposal.observation_id.clone()];
    refs.extend(
        proposal
            .evidence
            .iter()
            .map(|evidence| format!("{:?}:{}", evidence.source, evidence.reference)),
    );
    refs
}

pub(in crate::writer_agent::kernel::trace_recording) fn json_object_keys(
    value: &serde_json::Value,
) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

pub(in crate::writer_agent::kernel::trace_recording) fn context_pack_built_reports(
    context_pack: &WritingContextPack,
) -> Vec<WriterContextPackBuiltSourceReport> {
    context_pack
        .budget_report
        .source_reports
        .iter()
        .map(|source| WriterContextPackBuiltSourceReport {
            source: source.source.clone(),
            id: None,
            label: None,
            requested: Some(source.requested),
            original_chars: None,
            provided: source.provided,
            truncated: source.truncated,
            required: context_pack
                .task
                .required_source_budgets()
                .iter()
                .any(|(required, _)| format!("{:?}", required) == source.source),
            reason: Some(source.reason.clone()).filter(|reason| !reason.trim().is_empty()),
            truncation_reason: source.truncation_reason.clone(),
        })
        .collect()
}

pub(in crate::writer_agent::kernel::trace_recording) fn chapter_context_pack_built_reports(
    context: &crate::chapter_generation::BuiltChapterContext,
) -> Vec<WriterContextPackBuiltSourceReport> {
    context
        .sources
        .iter()
        .map(|source| WriterContextPackBuiltSourceReport {
            source: source.source_type.clone(),
            id: Some(source.id.clone()).filter(|id| !id.trim().is_empty()),
            label: Some(source.label.clone()).filter(|label| !label.trim().is_empty()),
            requested: None,
            original_chars: Some(source.original_chars),
            provided: source.included_chars,
            truncated: source.truncated,
            required: matches!(
                source.source_type.as_str(),
                "instruction"
                    | "outline"
                    | "target_beat"
                    | "previous_chapters"
                    | "lorebook"
                    | "project_brain"
            ),
            reason: None,
            truncation_reason: source.truncated.then(|| {
                format!(
                    "Chapter context budget included {} of {} chars.",
                    source.included_chars, source.original_chars
                )
            }),
        })
        .collect()
}
