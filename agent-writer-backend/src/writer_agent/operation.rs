//! WriterOperation — typed, inspectable, reversible editor operations.
//! Replaces XML action tags as the primary agent operation mechanism.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind")]
pub enum WriterOperation {
    #[serde(rename = "text.insert")]
    TextInsert {
        chapter: String,
        at: usize,
        text: String,
        revision: String,
    },
    #[serde(rename = "text.replace")]
    TextReplace {
        chapter: String,
        from: usize,
        to: usize,
        text: String,
        revision: String,
    },
    #[serde(rename = "text.annotate")]
    TextAnnotate {
        chapter: String,
        from: usize,
        to: usize,
        message: String,
        severity: AnnotationSeverity,
    },
    #[serde(rename = "canon.upsert_entity")]
    CanonUpsertEntity { entity: CanonEntityOp },
    #[serde(rename = "canon.update_attribute")]
    CanonUpdateAttribute {
        entity: String,
        attribute: String,
        value: String,
        confidence: f64,
    },
    #[serde(rename = "canon.upsert_rule")]
    CanonUpsertRule { rule: CanonRuleOp },
    #[serde(rename = "promise.add")]
    PromiseAdd { promise: PlotPromiseOp },
    #[serde(rename = "promise.resolve")]
    PromiseResolve {
        #[serde(rename = "promiseId")]
        promise_id: String,
        chapter: String,
    },
    #[serde(rename = "promise.defer")]
    PromiseDefer {
        #[serde(rename = "promiseId")]
        promise_id: String,
        chapter: String,
        #[serde(rename = "expectedPayoff")]
        expected_payoff: String,
    },
    #[serde(rename = "promise.abandon")]
    PromiseAbandon {
        #[serde(rename = "promiseId")]
        promise_id: String,
        chapter: String,
        reason: String,
    },
    #[serde(rename = "character.upsert")]
    CharacterUpsert {
        name: String,
        aliases: Vec<String>,
        #[serde(rename = "roleType")]
        role_type: String,
        summary: String,
    },
    #[serde(rename = "character_state.upsert")]
    CharacterStateUpsert {
        #[serde(rename = "characterId")]
        character_id: i64,
        #[serde(rename = "validFromChapter")]
        valid_from_chapter: String,
        #[serde(rename = "coreCommitments")]
        core_commitments: serde_json::Value,
        #[serde(rename = "goalState")]
        goal_state: serde_json::Value,
        #[serde(rename = "identityState")]
        identity_state: serde_json::Value,
        #[serde(rename = "sourceRef")]
        source_ref: String,
    },
    #[serde(rename = "relationship.upsert")]
    RelationshipUpsert {
        #[serde(rename = "characterAId")]
        character_a_id: i64,
        #[serde(rename = "characterBId")]
        character_b_id: i64,
        #[serde(rename = "relationType")]
        relation_type: String,
        visibility: String,
        #[serde(rename = "validFromChapter")]
        valid_from_chapter: String,
        #[serde(rename = "sourceRef")]
        source_ref: String,
    },
    #[serde(rename = "promise.bind_subject")]
    PromiseBindSubject {
        #[serde(rename = "promiseId")]
        promise_id: i64,
        #[serde(rename = "subjectIds")]
        subject_ids: Vec<i64>,
        #[serde(rename = "subjectType")]
        subject_type: String,
    },
    #[serde(rename = "style.update_preference")]
    StyleUpdatePreference { key: String, value: String },
    #[serde(rename = "story_contract.upsert")]
    StoryContractUpsert { contract: StoryContractOp },
    #[serde(rename = "chapter_mission.upsert")]
    ChapterMissionUpsert { mission: ChapterMissionOp },
    #[serde(rename = "outline.update")]
    OutlineUpdate {
        #[serde(rename = "nodeId")]
        node_id: String,
        patch: serde_json::Value,
    },
    #[serde(rename = "knowledge.upsert")]
    KnowledgeUpsert {
        topic: String,
        #[serde(rename = "truthState")]
        truth_state: String,
    },
    #[serde(rename = "knowledge_ownership.upsert")]
    KnowledgeOwnershipUpsert {
        #[serde(rename = "knowledgeId")]
        knowledge_id: i64,
        #[serde(rename = "holderType")]
        holder_type: String,
        #[serde(rename = "holderId")]
        holder_id: i64,
        #[serde(rename = "knowledgeMode")]
        knowledge_mode: String,
        #[serde(rename = "validFromChapter")]
        valid_from_chapter: String,
    },
    #[serde(rename = "identity_layer.upsert")]
    IdentityLayerUpsert {
        #[serde(rename = "characterId")]
        character_id: i64,
        #[serde(rename = "publicIdentity")]
        public_identity: String,
        #[serde(rename = "privateIdentity")]
        private_identity: String,
        #[serde(rename = "validFromChapter")]
        valid_from_chapter: String,
    },
    #[serde(rename = "reveal_event.record")]
    RevealEventRecord {
        #[serde(rename = "subjectId")]
        subject_id: i64,
        #[serde(rename = "revealType")]
        reveal_type: String,
        #[serde(rename = "revealedTo")]
        revealed_to: String,
        chapter: String,
    },
    #[serde(rename = "scene.upsert")]
    SceneUpsert {
        #[serde(rename = "chapterTitle")]
        chapter_title: String,
        sequence: i32,
        #[serde(rename = "sceneType")]
        scene_type: String,
        summary: String,
    },
    #[serde(rename = "scene_state.upsert")]
    SceneStateUpsert {
        #[serde(rename = "sceneId")]
        scene_id: i64,
        objective: String,
        participants: Vec<String>,
        #[serde(rename = "locationRef")]
        location_ref: String,
        #[serde(rename = "entryState")]
        entry_state: serde_json::Value,
        #[serde(rename = "exitState")]
        exit_state: serde_json::Value,
    },
    #[serde(rename = "scene_obligation.upsert")]
    SceneObligationUpsert {
        #[serde(rename = "sceneId")]
        scene_id: i64,
        #[serde(rename = "promiseIds")]
        promise_ids: Vec<i64>,
        #[serde(rename = "missionRefs")]
        mission_refs: Vec<String>,
        #[serde(rename = "payoffTargets")]
        payoff_targets: Vec<String>,
    },
    #[serde(rename = "scene_result.record")]
    SceneResultRecord {
        #[serde(rename = "sceneId")]
        scene_id: i64,
        outcome: String,
        consequence: String,
    },
    #[serde(rename = "time_slice.upsert")]
    TimeSliceUpsert { label: String, relative_order: i32 },
    #[serde(rename = "chapter_time_mapping.upsert")]
    ChapterTimeMappingUpsert {
        chapter_title: String,
        scene_id: Option<i64>,
        time_slice_id: i64,
        narrative_mode: String,
    },
    #[serde(rename = "timeline_event.record")]
    TimelineEventRecord {
        subject_ids: Vec<i64>,
        event_type: String,
        time_slice_id: i64,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonEntityOp {
    pub kind: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub summary: String,
    pub attributes: serde_json::Value,
    pub confidence: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonRuleOp {
    pub rule: String,
    pub category: String,
    pub priority: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlotPromiseOp {
    pub kind: String,
    pub title: String,
    pub description: String,
    pub introduced_chapter: String,
    pub expected_payoff: String,
    pub priority: i32,
    pub related_entities: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryContractOp {
    pub project_id: String,
    pub title: String,
    pub genre: String,
    pub target_reader: String,
    pub reader_promise: String,
    pub first_30_chapter_promise: String,
    pub main_conflict: String,
    pub structural_boundary: String,
    pub tone_contract: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterMissionOp {
    pub project_id: String,
    pub chapter_title: String,
    pub mission: String,
    pub must_include: String,
    pub must_not: String,
    pub expected_ending: String,
    pub status: String,
    pub source_ref: String,
    #[serde(default)]
    pub blocked_reason: String,
    #[serde(default)]
    pub retired_history: String,
}

/// Result of executing a WriterOperation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub success: bool,
    pub operation: WriterOperation,
    pub error: Option<OperationError>,
    pub revision_after: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationApproval {
    pub source: String,
    pub actor: String,
    pub reason: String,
    pub proposal_id: Option<String>,
    pub surfaced_to_user: bool,
    pub created_at: u64,
}

impl OperationApproval {
    pub fn is_valid_for_write(&self) -> bool {
        !self.source.trim().is_empty()
            && !self.actor.trim().is_empty()
            && !self.reason.trim().is_empty()
            && self.surfaced_to_user
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    pub code: String,
    pub message: String,
}

impl OperationError {
    pub fn conflict(msg: &str) -> Self {
        Self {
            code: "conflict".into(),
            message: msg.into(),
        }
    }
    pub fn invalid(msg: &str) -> Self {
        Self {
            code: "invalid".into(),
            message: msg.into(),
        }
    }

    pub fn approval_required(msg: &str) -> Self {
        Self {
            code: "approval_required".into(),
            message: msg.into(),
        }
    }
}

/// Executes text operations with revision checks.
/// Rejects operations where the chapter revision doesn't match (preventing lost updates).
pub fn execute_text_operation(
    op: &WriterOperation,
    current_content: &str,
    current_revision: &str,
) -> Result<(String, String), OperationError> {
    match op {
        WriterOperation::TextInsert {
            revision,
            chapter: _,
            at,
            text,
        } => {
            if revision != current_revision {
                return Err(OperationError::conflict(
                    "Chapter was modified since the proposal was created",
                ));
            }
            let mut chars: Vec<char> = current_content.chars().collect();
            let pos = (*at).min(chars.len());
            let new_text: Vec<char> = text.chars().collect();
            chars.splice(pos..pos, new_text);
            let new_content: String = chars.into_iter().collect();
            let new_revision = crate::storage::content_revision(&new_content);
            Ok((new_content, new_revision))
        }
        WriterOperation::TextReplace {
            revision,
            chapter: _,
            from,
            to,
            text,
        } => {
            if revision != current_revision {
                return Err(OperationError::conflict(
                    "Chapter was modified since the proposal was created",
                ));
            }
            let mut chars: Vec<char> = current_content.chars().collect();
            let start = (*from).min(chars.len());
            let end = (*to).min(chars.len());
            let new_text: Vec<char> = text.chars().collect();
            chars.splice(start..end, new_text);
            let new_content: String = chars.into_iter().collect();
            let new_revision = crate::storage::content_revision(&new_content);
            Ok((new_content, new_revision))
        }
        _ => Err(OperationError::invalid("Not a text operation")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_insert_updates_revision() {
        let op = WriterOperation::TextInsert {
            chapter: "ch1".into(),
            at: 5,
            text: "hello".into(),
            revision: "abc".into(),
        };
        let (content, rev) = execute_text_operation(&op, "1234567890", "abc").unwrap();
        assert_eq!(content, "12345hello67890");
        assert_ne!(rev, "abc"); // revision changed
    }

    #[test]
    fn test_text_replace_rejects_wrong_revision() {
        let op = WriterOperation::TextReplace {
            chapter: "ch1".into(),
            from: 0,
            to: 3,
            text: "x".into(),
            revision: "wrong".into(),
        };
        let result = execute_text_operation(&op, "abcdef", "correct");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "conflict");
    }

    #[test]
    fn test_text_replace_succeeds_with_correct_revision() {
        let op = WriterOperation::TextReplace {
            chapter: "ch1".into(),
            from: 3,
            to: 6,
            text: "XYZ".into(),
            revision: "r1".into(),
        };
        let (content, _) = execute_text_operation(&op, "abcdefgh", "r1").unwrap();
        assert_eq!(content, "abcXYZgh");
    }
}
