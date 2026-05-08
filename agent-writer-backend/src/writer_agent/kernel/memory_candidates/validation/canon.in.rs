pub fn validate_canon_candidate(candidate: &CanonEntityOp) -> MemoryCandidateQuality {
    let name = candidate.name.trim();
    if name.chars().count() < 2 {
        return MemoryCandidateQuality::Vague {
            reason: "entity name too short (min 2 chars)".to_string(),
        };
    }
    let summary = candidate.summary.trim();
    if summary.chars().count() < 8 {
        return MemoryCandidateQuality::Vague {
            reason: format!(
                "entity summary too short ({} chars, min 8)",
                summary.chars().count()
            ),
        };
    }
    MemoryCandidateQuality::Acceptable
}

pub fn validate_canon_candidate_with_memory(
    candidate: &CanonEntityOp,
    memory: &WriterMemory,
) -> MemoryCandidateQuality {
    let quality = validate_canon_candidate(candidate);
    if quality != MemoryCandidateQuality::Acceptable {
        return quality;
    }

    let Some(existing) = find_existing_canon_entity(candidate, memory) else {
        return MemoryCandidateQuality::Acceptable;
    };

    if existing.kind.trim() != candidate.kind.trim() {
        return MemoryCandidateQuality::Conflict {
            existing_name: existing.name.clone(),
            reason: format!(
                "kind differs for existing canon '{}': existing={}, candidate={}",
                existing.name, existing.kind, candidate.kind
            ),
        };
    }

    if let Some((attribute, existing_value, candidate_value)) =
        conflicting_canon_attribute(candidate, &existing)
    {
        return MemoryCandidateQuality::Conflict {
            existing_name: existing.name.clone(),
            reason: format!(
                "{}.{} conflicts: existing={}, candidate={}",
                existing.name, attribute, existing_value, candidate_value
            ),
        };
    }

    let mergeable_attributes = mergeable_canon_attributes(candidate, &existing);
    if !mergeable_attributes.is_empty() {
        return MemoryCandidateQuality::MergeableAttributes {
            existing_name: existing.name,
            attributes: mergeable_attributes,
        };
    }

    MemoryCandidateQuality::Duplicate {
        existing_name: existing.name,
    }
}

fn find_existing_canon_entity(
    candidate: &CanonEntityOp,
    memory: &WriterMemory,
) -> Option<CanonEntitySummary> {
    let mut names = Vec::with_capacity(candidate.aliases.len() + 1);
    names.push(candidate.name.trim().to_string());
    names.extend(
        candidate
            .aliases
            .iter()
            .map(|alias| alias.trim().to_string()),
    );

    let resolved = names
        .into_iter()
        .filter(|name| !name.is_empty())
        .filter_map(|name| memory.resolve_canon_entity_name(&name).ok().flatten())
        .collect::<HashSet<_>>();
    if resolved.is_empty() {
        return None;
    }

    memory
        .list_canon_entities()
        .ok()?
        .into_iter()
        .find(|entity| resolved.contains(&entity.name))
}

fn conflicting_canon_attribute(
    candidate: &CanonEntityOp,
    existing: &CanonEntitySummary,
) -> Option<(String, String, String)> {
    let candidate_attributes = candidate.attributes.as_object()?;
    let existing_attributes = existing.attributes.as_object()?;

    for (attribute, candidate_value) in candidate_attributes {
        let Some(candidate_text) = canon_attribute_value(candidate_value) else {
            continue;
        };
        let Some(existing_text) = existing_attributes
            .get(attribute)
            .and_then(canon_attribute_value)
        else {
            continue;
        };
        if existing_text != candidate_text {
            return Some((attribute.clone(), existing_text, candidate_text));
        }
    }

    None
}

fn mergeable_canon_attributes(
    candidate: &CanonEntityOp,
    existing: &CanonEntitySummary,
) -> Vec<(String, String)> {
    let Some(candidate_attributes) = candidate.attributes.as_object() else {
        return Vec::new();
    };
    let existing_attributes = existing.attributes.as_object().cloned().unwrap_or_default();

    candidate_attributes
        .iter()
        .filter_map(|(attribute, candidate_value)| {
            let candidate_text = canon_attribute_value(candidate_value)?;
            if existing_attributes.contains_key(attribute) {
                return None;
            }
            Some((attribute.clone(), candidate_text))
        })
        .collect()
}

fn canon_attribute_value(value: &serde_json::Value) -> Option<String> {
    let text = match value {
        serde_json::Value::Null => return None,
        serde_json::Value::String(value) => value.trim().to_string(),
        serde_json::Value::Array(values) if values.is_empty() => return None,
        serde_json::Value::Object(values) if values.is_empty() => return None,
        other => other.to_string(),
    };

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
