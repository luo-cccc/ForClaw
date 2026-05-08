pub(crate) fn memory_candidates_from_observation(
    observation: &WriterObservation,
    memory: &WriterMemory,
    observation_id: &str,
    proposal_counter: &mut u64,
    session_id: &str,
) -> Vec<AgentProposal> {
    let mut proposals = Vec::new();
    let mut known = memory.get_canon_entity_names().unwrap_or_default();
    known.sort();
    known.dedup();

    let feedback = MemoryExtractionFeedback::from_memory(memory);

    for mut entity in extract_new_canon_entities(&observation.paragraph, &known)
        .into_iter()
        .take(3)
    {
        let slot = memory_candidate_slot_for_canon(&entity);
        if feedback.is_suppressed(&slot) {
            continue;
        }
        if feedback.is_preferred(&slot) {
            entity.confidence = (entity.confidence + 0.08).min(0.92);
        }
        match validate_canon_candidate_with_memory(&entity, memory) {
            MemoryCandidateQuality::Acceptable => proposals.push(canon_candidate_proposal(
                observation,
                observation_id,
                proposal_counter,
                session_id,
                entity,
                CandidateSource::Local,
            )),
            MemoryCandidateQuality::Conflict {
                existing_name,
                reason,
            } => proposals.push(canon_conflict_candidate_proposal(
                observation,
                observation_id,
                proposal_counter,
                session_id,
                entity,
                existing_name,
                reason,
                CandidateSource::Local,
            )),
            MemoryCandidateQuality::MergeableAttributes {
                existing_name,
                attributes,
            } => proposals.push(canon_attribute_merge_candidate_proposal(
                observation,
                observation_id,
                proposal_counter,
                session_id,
                existing_name,
                attributes,
                entity.confidence,
                CandidateSource::Local,
            )),
            MemoryCandidateQuality::Vague { .. } | MemoryCandidateQuality::Duplicate { .. } => {}
        }
    }

    for mut promise in extract_plot_promises(&observation.paragraph, observation)
        .into_iter()
        .take(3)
    {
        let slot = memory_candidate_slot_for_promise(&promise);
        if feedback.is_suppressed(&slot) {
            continue;
        }
        if feedback.is_preferred(&slot) {
            promise.priority = (promise.priority + 1).min(10);
        }
        if validate_promise_candidate_with_dedup(&promise, memory)
            == MemoryCandidateQuality::Acceptable
        {
            proposals.push(promise_candidate_proposal(
                observation,
                observation_id,
                proposal_counter,
                session_id,
                promise,
                CandidateSource::Local,
            ));
        }
    }

    proposals
}

pub(crate) fn canon_candidate_proposal(
    observation: &WriterObservation,
    observation_id: &str,
    proposal_counter: &mut u64,
    session_id: &str,
    entity: CanonEntityOp,
    source: CandidateSource,
) -> AgentProposal {
    let id = proposal_id(session_id, *proposal_counter);
    *proposal_counter += 1;
    let preview = format!("沉淀设定: {} - {}", entity.name, entity.summary);
    let snippet = entity.summary.clone();
    let (rationale, confidence, risks) = source.canon_metadata();
    AgentProposal {
        id,
        observation_id: observation_id.to_string(),
        kind: ProposalKind::CanonUpdate,
        priority: ProposalPriority::Ambient,
        target: observation.cursor.clone(),
        preview,
        operations: vec![WriterOperation::CanonUpsertEntity { entity }],
        rationale,
        evidence: vec![EvidenceRef {
            source: EvidenceSource::ChapterText,
            reference: observation
                .chapter_title
                .clone()
                .unwrap_or_else(|| "current chapter".to_string()),
            snippet,
        }],
        risks,
        alternatives: vec![],
        confidence,
        expires_at: None,
    }
}

pub(crate) fn promise_candidate_proposal(
    observation: &WriterObservation,
    observation_id: &str,
    proposal_counter: &mut u64,
    session_id: &str,
    promise: PlotPromiseOp,
    source: CandidateSource,
) -> AgentProposal {
    let id = proposal_id(session_id, *proposal_counter);
    *proposal_counter += 1;
    let preview = format!("登记伏笔: {} - {}", promise.title, promise.description);
    let snippet = promise.description.clone();
    let (rationale, confidence, risks) = source.promise_metadata();
    AgentProposal {
        id,
        observation_id: observation_id.to_string(),
        kind: ProposalKind::PlotPromise,
        priority: ProposalPriority::Ambient,
        target: observation.cursor.clone(),
        preview,
        operations: vec![WriterOperation::PromiseAdd { promise }],
        rationale,
        evidence: vec![EvidenceRef {
            source: EvidenceSource::ChapterText,
            reference: observation
                .chapter_title
                .clone()
                .unwrap_or_else(|| "current chapter".to_string()),
            snippet,
        }],
        risks,
        alternatives: vec![],
        confidence,
        expires_at: None,
    }
}

pub(crate) fn canon_conflict_candidate_proposal(
    observation: &WriterObservation,
    observation_id: &str,
    proposal_counter: &mut u64,
    session_id: &str,
    entity: CanonEntityOp,
    existing_name: String,
    reason: String,
    source: CandidateSource,
) -> AgentProposal {
    let id = proposal_id(session_id, *proposal_counter);
    *proposal_counter += 1;
    let preview = format!("设定冲突需确认: {} - {}", entity.name, reason);
    let source_label = source.label();
    AgentProposal {
        id,
        observation_id: observation_id.to_string(),
        kind: ProposalKind::ContinuityWarning,
        priority: ProposalPriority::Urgent,
        target: observation.cursor.clone(),
        preview,
        operations: vec![],
        rationale: format!(
            "{} 记忆候选与现有 canon 冲突，必须由作者明确确认后再进入长期记忆。",
            source_label
        ),
        evidence: vec![
            EvidenceRef {
                source: EvidenceSource::ChapterText,
                reference: observation
                    .chapter_title
                    .clone()
                    .unwrap_or_else(|| "current chapter".to_string()),
                snippet: entity.summary.clone(),
            },
            EvidenceRef {
                source: EvidenceSource::Canon,
                reference: existing_name,
                snippet: reason,
            },
        ],
        risks: vec![
            "未自动写入长期 canon；请先确认是正文临场描述、误抽取，还是需要修改既有设定。"
                .to_string(),
        ],
        alternatives: vec![],
        confidence: match source {
            CandidateSource::Local => 0.7,
            CandidateSource::Llm(_) => 0.82,
        },
        expires_at: None,
    }
}

pub(crate) fn canon_attribute_merge_candidate_proposal(
    observation: &WriterObservation,
    observation_id: &str,
    proposal_counter: &mut u64,
    session_id: &str,
    existing_name: String,
    attributes: Vec<(String, String)>,
    confidence: f64,
    source: CandidateSource,
) -> AgentProposal {
    let id = proposal_id(session_id, *proposal_counter);
    *proposal_counter += 1;
    let source_label = source.label();
    let attribute_text = attributes
        .iter()
        .map(|(key, value)| format!("{}.{} = {}", existing_name, key, value))
        .collect::<Vec<_>>()
        .join("; ");
    let operations = attributes
        .iter()
        .map(|(attribute, value)| WriterOperation::CanonUpdateAttribute {
            entity: existing_name.clone(),
            attribute: attribute.clone(),
            value: value.clone(),
            confidence,
        })
        .collect::<Vec<_>>();
    AgentProposal {
        id,
        observation_id: observation_id.to_string(),
        kind: ProposalKind::CanonUpdate,
        priority: ProposalPriority::Ambient,
        target: observation.cursor.clone(),
        preview: format!("补充设定属性: {}", attribute_text),
        operations,
        rationale: format!(
            "{} 记忆候选命中既有 canon，只补充缺失属性；需作者确认后合并。",
            source_label
        ),
        evidence: vec![
            EvidenceRef {
                source: EvidenceSource::ChapterText,
                reference: observation
                    .chapter_title
                    .clone()
                    .unwrap_or_else(|| "current chapter".to_string()),
                snippet: attribute_text.clone(),
            },
            EvidenceRef {
                source: EvidenceSource::Canon,
                reference: existing_name,
                snippet: "existing entity; missing non-conflicting attributes only".to_string(),
            },
        ],
        risks: vec!["仅补充缺失属性，不覆盖既有 canon；请确认该属性是长期设定。".to_string()],
        alternatives: vec![],
        confidence: match source {
            CandidateSource::Local => 0.64,
            CandidateSource::Llm(_) => 0.8,
        },
        expires_at: None,
    }
}
