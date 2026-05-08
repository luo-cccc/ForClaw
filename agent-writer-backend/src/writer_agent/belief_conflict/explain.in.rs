fn explain_forbidden_reveals(evidence: &[BeliefEvidence]) -> Vec<BeliefConflictExplanation> {
    let guards = evidence
        .iter()
        .filter_map(classify_guard)
        .collect::<Vec<_>>();
    let mut conflicts = Vec::new();

    for reveal in evidence.iter().filter(|item| is_reveal_claim(item)) {
        let matched_guards = guards
            .iter()
            .filter(|guard| guard.evidence.reference != reveal.reference)
            .filter(|guard| terms_overlap(&guard.terms, &reveal.snippet))
            .collect::<Vec<_>>();
        if matched_guards.is_empty() {
            continue;
        }

        let mut conflict_evidence = Vec::new();
        for guard in matched_guards {
            let mut item = guard.evidence.clone();
            item.signals.push(match guard.signal {
                GuardSignal::Forbidden => "guard=forbidden_reveal".to_string(),
                GuardSignal::DeferredPayoff => "guard=deferred_payoff".to_string(),
            });
            conflict_evidence.push(item);
        }
        let mut reveal_item = reveal.clone();
        reveal_item
            .signals
            .push("claim=revealed_or_resolved".to_string());
        conflict_evidence.push(reveal_item);
        let confidence = conflict_confidence(&conflict_evidence);
        let summary = format!(
            "Reveal claim conflicts with {} guarded belief source(s).",
            conflict_evidence.len().saturating_sub(1)
        );

        conflicts.push(BeliefConflictExplanation {
            id: stable_conflict_id(BeliefConflictKind::ForbiddenReveal, &conflict_evidence),
            kind: BeliefConflictKind::ForbiddenReveal,
            summary,
            rationale:
                "A source says this information is forbidden or deferred, while another source says it has already been revealed or resolved."
                    .to_string(),
            confidence,
            evidence: conflict_evidence,
            resolution_hint:
                "Ask the author to confirm whether to update the guard, move the reveal later, or archive the stale source."
                    .to_string(),
        });
    }

    conflicts
}

fn classify_guard(evidence: &BeliefEvidence) -> Option<GuardBelief<'_>> {
    let text = evidence.snippet.trim();
    if text.is_empty() {
        return None;
    }
    if has_forbid_signal(text) {
        let terms = guard_terms(text);
        if !terms.is_empty() {
            return Some(GuardBelief {
                evidence,
                terms,
                signal: GuardSignal::Forbidden,
            });
        }
    }
    if evidence.source == BeliefSource::PromiseLedger && has_deferred_payoff_signal(text) {
        let terms = guard_terms(text);
        if !terms.is_empty() {
            return Some(GuardBelief {
                evidence,
                terms,
                signal: GuardSignal::DeferredPayoff,
            });
        }
    }
    None
}

fn explain_fact_contradictions(evidence: &[BeliefEvidence]) -> Vec<BeliefConflictExplanation> {
    let facts = evidence
        .iter()
        .flat_map(extract_facts)
        .collect::<Vec<FactBelief<'_>>>();
    let mut conflicts = Vec::new();

    for left_index in 0..facts.len() {
        for right_index in (left_index + 1)..facts.len() {
            let left = &facts[left_index];
            let right = &facts[right_index];
            if left.evidence.reference == right.evidence.reference
                || left.subject != right.subject
                || left.predicate != right.predicate
                || !objects_conflict(&left.object, &right.object)
            {
                continue;
            }
            let mut conflict_evidence = vec![left.evidence.clone(), right.evidence.clone()];
            for item in &mut conflict_evidence {
                item.signals
                    .push(format!("fact={}:{}", left.subject, left.predicate));
            }
            conflicts.push(BeliefConflictExplanation {
                id: stable_conflict_id(BeliefConflictKind::FactContradiction, &conflict_evidence),
                kind: BeliefConflictKind::FactContradiction,
                summary: format!("Conflicting facts for {} {}.", left.subject, left.predicate),
                rationale: format!(
                    "One source says '{}', while another source says '{}'.",
                    left.object, right.object
                ),
                confidence: conflict_confidence(&conflict_evidence),
                evidence: conflict_evidence,
                resolution_hint:
                    "Keep both sources visible until the author confirms which fact is current."
                        .to_string(),
            });
        }
    }

    conflicts
}

