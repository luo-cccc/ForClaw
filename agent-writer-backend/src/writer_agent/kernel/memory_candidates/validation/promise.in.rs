pub fn validate_promise_candidate(candidate: &PlotPromiseOp) -> MemoryCandidateQuality {
    let title = candidate.title.trim();
    if title.chars().count() < 2 {
        return MemoryCandidateQuality::Vague {
            reason: "promise title too short (min 2 chars)".to_string(),
        };
    }
    let description = candidate.description.trim();
    if description.chars().count() < 8 {
        return MemoryCandidateQuality::Vague {
            reason: format!(
                "promise description too short ({} chars, min 8)",
                description.chars().count()
            ),
        };
    }
    MemoryCandidateQuality::Acceptable
}

pub fn validate_promise_candidate_with_dedup(
    candidate: &PlotPromiseOp,
    memory: &WriterMemory,
) -> MemoryCandidateQuality {
    let quality = validate_promise_candidate(candidate);
    if quality != MemoryCandidateQuality::Acceptable {
        return quality;
    }
    if let Ok(existing) = memory.get_open_promise_summaries() {
        if existing
            .iter()
            .any(|p| {
                crate::writer_agent::memory::promise_identity_matches(
                    &candidate.kind,
                    &candidate.title,
                    &candidate.description,
                    &p.kind,
                    &p.title,
                    &p.description,
                )
            })
        {
            return MemoryCandidateQuality::Duplicate {
                existing_name: candidate.title.clone(),
            };
        }
    }
    MemoryCandidateQuality::Acceptable
}
