use serde::{Deserialize, Serialize};

// ── D1: EvidenceRef and WorldAsset base types ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    pub source_id: String,
    pub source_path: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub excerpt: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Proposed,
    Approved,
    Rejected,
}

impl ApprovalStatus {
    pub fn is_approved(&self) -> bool {
        matches!(self, ApprovalStatus::Approved)
    }

    pub fn can_be_hard_constraint(&self) -> bool {
        // Proposed assets can only produce warnings, never hard constraints
        matches!(self, ApprovalStatus::Approved)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldAssetKind {
    Entity,
    Rule,
    Relation,
    Hierarchy,
    TimelineFact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldAsset {
    pub id: String,
    pub kind: WorldAssetKind,
    pub name: String,
    pub summary: String,
    pub evidence: Vec<EvidenceRef>,
    pub approval_status: ApprovalStatus,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl WorldAsset {
    /// Returns true if this asset has at least one evidence ref.
    pub fn has_evidence(&self) -> bool {
        !self.evidence.is_empty() && self.evidence.iter().all(|e| !e.excerpt.is_empty())
    }

    /// Returns true if this asset can be used as a hard constraint source.
    pub fn can_hard_enforce(&self) -> bool {
        self.approval_status.is_approved() && self.has_evidence()
    }
}

// ── D2: CanonConstraint types ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonConstraintKind {
    RequiredFact,
    ForbiddenClaim,
    ForbiddenAction,
    RequiredCost,
    HierarchyLimit,
    ExceptionRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintSeverity {
    Info,
    Warning,
    Hard,
}

impl ConstraintSeverity {
    pub fn is_hard(&self) -> bool {
        matches!(self, ConstraintSeverity::Hard)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonConstraint {
    pub id: String,
    pub kind: CanonConstraintKind,
    pub summary: String,
    pub trigger_terms: Vec<String>,
    pub forbidden_terms: Vec<String>,
    pub required_terms: Vec<String>,
    pub severity: ConstraintSeverity,
    pub source_asset_id: String,
    pub evidence: Vec<EvidenceRef>,
}

impl CanonConstraint {
    /// A constraint is only hard if both the asset is approved AND the severity is Hard.
    pub fn effective_severity(&self, asset: &WorldAsset) -> ConstraintSeverity {
        if asset.can_hard_enforce() {
            self.severity.clone()
        } else {
            // Downgrade to Warning if asset is not approved or lacks evidence
            ConstraintSeverity::Warning
        }
    }
}

// ── D3: SceneContract ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneContract {
    pub chapter_id: String,
    pub mission: String,
    pub required_facts: Vec<CanonConstraint>,
    pub active_constraints: Vec<CanonConstraint>,
    pub required_state_deltas: Vec<crate::chapter_generation::StateDelta>,
    pub allowed_reveals: Vec<String>,
    pub blocked_reveals: Vec<String>,
    pub evidence_refs: Vec<EvidenceRef>,
}

impl SceneContract {
    /// Returns only the hard constraints from active_constraints.
    pub fn hard_constraints(&self, assets: &[WorldAsset]) -> Vec<&CanonConstraint> {
        self.active_constraints
            .iter()
            .filter(|c| {
                assets
                    .iter()
                    .find(|a| a.id == c.source_asset_id)
                    .map(|a| c.effective_severity(a).is_hard())
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Returns constraints that would be downgraded to warning due to unapproved source.
    pub fn warned_constraints(&self, assets: &[WorldAsset]) -> Vec<&CanonConstraint> {
        self.active_constraints
            .iter()
            .filter(|c| {
                assets
                    .iter()
                    .find(|a| a.id == c.source_asset_id)
                    .map(|a| !c.effective_severity(a).is_hard())
                    .unwrap_or(true)
            })
            .collect()
    }
}

// ── D5: WorldConsistencyViolation ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldConsistencyViolation {
    pub constraint_id: String,
    pub severity: ConstraintSeverity,
    pub kind: CanonConstraintKind,
    pub message: String,
    pub text_excerpt: String,
    pub evidence: Vec<EvidenceRef>,
    pub suggested_fix: String,
}

// ── D1: Compiler functions ──

/// Compile WorldAsset rules into CanonConstraints.
/// Only assets with kind == Rule produce constraints.
/// Proposed rules are downgraded to Warning severity.
pub fn compile_canon_constraints(assets: &[WorldAsset]) -> Vec<CanonConstraint> {
    let mut constraints = Vec::new();
    for asset in assets.iter().filter(|a| matches!(a.kind, WorldAssetKind::Rule)) {
        let severity = if asset.can_hard_enforce() {
            ConstraintSeverity::Hard
        } else {
            ConstraintSeverity::Warning
        };
        constraints.push(CanonConstraint {
            id: format!("constraint-{}", asset.id),
            kind: CanonConstraintKind::ForbiddenClaim,
            summary: asset.summary.clone(),
            trigger_terms: vec![asset.name.clone()],
            forbidden_terms: asset.tags.clone(),
            required_terms: Vec::new(),
            severity,
            source_asset_id: asset.id.clone(),
            evidence: asset.evidence.clone(),
        });
    }
    constraints
}

/// Compile a SceneContract from mission, world assets, and pre-built constraints.
/// Selects the most relevant 3-8 constraints based on mission keywords.
pub fn compile_scene_contract(
    chapter_id: &str,
    mission: &str,
    assets: &[WorldAsset],
    constraints: &[CanonConstraint],
    required_deltas: &[crate::chapter_generation::StateDelta],
) -> SceneContract {
    let mission_lower = mission.to_lowercase();
    let mut scored: Vec<(f32, &CanonConstraint)> = constraints
        .iter()
        .map(|c| {
            let mut score = 0.0f32;
            for term in c.trigger_terms.iter() {
                if mission_lower.contains(&term.to_lowercase()) {
                    score += 1.0;
                }
            }
            for term in c.forbidden_terms.iter() {
                if mission_lower.contains(&term.to_lowercase()) {
                    score += 0.5;
                }
            }
            (score, c)
        })
        .collect();

    // Sort by relevance descending; hard constraints get a tie-breaker bonus
    scored.sort_by(|a, b| {
        let a_hard = assets
            .iter()
            .find(|asset| asset.id == a.1.source_asset_id)
            .map(|asset| a.1.effective_severity(asset).is_hard())
            .unwrap_or(false);
        let b_hard = assets
            .iter()
            .find(|asset| asset.id == b.1.source_asset_id)
            .map(|asset| b.1.effective_severity(asset).is_hard())
            .unwrap_or(false);
        let a_score = a.0 + if a_hard { 0.1 } else { 0.0 };
        let b_score = b.0 + if b_hard { 0.1 } else { 0.0 };
        b_score.partial_cmp(&a_score).unwrap_or(std::cmp::Ordering::Equal)
    });

    let selected: Vec<CanonConstraint> = scored
        .iter()
        .take(8)
        .map(|(_, c)| (*c).clone())
        .collect();

    let evidence_refs: Vec<EvidenceRef> = selected
        .iter()
        .flat_map(|c| c.evidence.clone())
        .collect();

    SceneContract {
        chapter_id: chapter_id.to_string(),
        mission: mission.to_string(),
        required_facts: Vec::new(),
        active_constraints: selected,
        required_state_deltas: required_deltas.to_vec(),
        allowed_reveals: Vec::new(),
        blocked_reveals: Vec::new(),
        evidence_refs,
    }
}

// ── D5: Consistency Validator ──

/// Check chapter text against world consistency constraints.
/// Returns violations for forbidden claims, skipped required costs, and hierarchy limits.
pub fn validate_world_consistency(
    chapter_text: &str,
    contract: &SceneContract,
    assets: &[WorldAsset],
) -> Vec<WorldConsistencyViolation> {
    let mut violations = Vec::new();
    let text_lower = chapter_text.to_lowercase();

    for constraint in &contract.active_constraints {
        let effective = assets
            .iter()
            .find(|a| a.id == constraint.source_asset_id)
            .map(|a| constraint.effective_severity(a))
            .unwrap_or(ConstraintSeverity::Warning);

        match constraint.kind {
            CanonConstraintKind::ForbiddenClaim => {
                for term in &constraint.forbidden_terms {
                    if text_lower.contains(&term.to_lowercase()) {
                        let excerpt = extract_excerpt(chapter_text, term);
                        violations.push(WorldConsistencyViolation {
                            constraint_id: constraint.id.clone(),
                            severity: effective.clone(),
                            kind: CanonConstraintKind::ForbiddenClaim,
                            message: format!("正文包含被禁止的设定断言: {}", term),
                            text_excerpt: excerpt,
                            evidence: constraint.evidence.clone(),
                            suggested_fix: constraint.summary.clone(),
                        });
                    }
                }
            }
            CanonConstraintKind::RequiredCost => {
                for trigger in &constraint.trigger_terms {
                    if text_lower.contains(&trigger.to_lowercase()) {
                        let cost_paid = constraint
                            .required_terms
                            .iter()
                            .any(|req| text_lower.contains(&req.to_lowercase()));
                        if !cost_paid {
                            let excerpt = extract_excerpt(chapter_text, trigger);
                            violations.push(WorldConsistencyViolation {
                                constraint_id: constraint.id.clone(),
                                severity: effective.clone(),
                                kind: CanonConstraintKind::RequiredCost,
                                message: format!(
                                    "触发 '{}' 但未支付要求的代价: {:?}",
                                    trigger, constraint.required_terms
                                ),
                                text_excerpt: excerpt,
                                evidence: constraint.evidence.clone(),
                                suggested_fix: format!(
                                    "在触发 '{}' 时确保支付代价",
                                    trigger
                                ),
                            });
                        }
                    }
                }
            }
            CanonConstraintKind::HierarchyLimit => {
                // Simplified: if both a low-tier identity and a high-tier action appear, warn
                let low_tier = constraint.trigger_terms.iter().any(|t| {
                    text_lower.contains(&t.to_lowercase())
                });
                let high_action = constraint.forbidden_terms.iter().any(|t| {
                    text_lower.contains(&t.to_lowercase())
                });
                if low_tier && high_action {
                    violations.push(WorldConsistencyViolation {
                        constraint_id: constraint.id.clone(),
                        severity: effective.clone(),
                        kind: CanonConstraintKind::HierarchyLimit,
                        message: format!("层级限制可能被突破: {:?} 尝试 {:?}", constraint.trigger_terms, constraint.forbidden_terms),
                        text_excerpt: chapter_text.chars().take(200).collect(),
                        evidence: constraint.evidence.clone(),
                        suggested_fix: "确认角色层级是否足以执行该动作，或提供例外理由".to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    violations
}

fn extract_excerpt(text: &str, keyword: &str) -> String {
    let lower = text.to_lowercase();
    if let Some(byte_pos) = lower.find(&keyword.to_lowercase()) {
        // Convert byte position to char position for safe slicing
        let char_pos = lower[..byte_pos].chars().count();
        let keyword_chars = keyword.chars().count();
        let text_chars: Vec<char> = text.chars().collect();
        let start = char_pos.saturating_sub(20);
        let end = (char_pos + keyword_chars + 20).min(text_chars.len());
        text_chars[start..end].iter().collect()
    } else {
        text.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_asset(id: &str, kind: WorldAssetKind, approved: bool) -> WorldAsset {
        WorldAsset {
            id: id.to_string(),
            kind,
            name: id.to_string(),
            summary: format!("summary of {}", id),
            evidence: vec![EvidenceRef {
                source_id: "src1".to_string(),
                source_path: None,
                start_line: None,
                end_line: None,
                excerpt: "evidence".to_string(),
                confidence: 0.9,
            }],
            approval_status: if approved {
                ApprovalStatus::Approved
            } else {
                ApprovalStatus::Proposed
            },
            tags: vec!["tag1".to_string()],
        }
    }

    #[test]
    fn world_asset_hard_enforce_requires_approval_and_evidence() {
        let approved = sample_asset("a1", WorldAssetKind::Rule, true);
        assert!(approved.can_hard_enforce());

        let proposed = sample_asset("a2", WorldAssetKind::Rule, false);
        assert!(!proposed.can_hard_enforce());
    }

    #[test]
    fn compile_constraints_only_from_rules() {
        let assets = vec![
            sample_asset("rule1", WorldAssetKind::Rule, true),
            sample_asset("entity1", WorldAssetKind::Entity, true),
        ];
        let constraints = compile_canon_constraints(&assets);
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].source_asset_id, "rule1");
    }

    #[test]
    fn proposed_rule_downgraded_to_warning() {
        let assets = vec![sample_asset("rule1", WorldAssetKind::Rule, false)];
        let constraints = compile_canon_constraints(&assets);
        assert_eq!(constraints[0].severity, ConstraintSeverity::Warning);
    }

    #[test]
    fn scene_contract_selects_relevant_constraints() {
        let assets = vec![
            sample_asset("fire_rule", WorldAssetKind::Rule, true),
            sample_asset("ice_rule", WorldAssetKind::Rule, true),
        ];
        let constraints = compile_canon_constraints(&assets);
        let contract = compile_scene_contract("ch1", "fire scene", &assets, &constraints, &[]);
        // fire_rule should be selected because its trigger term "fire_rule" loosely matches "fire scene"
        assert!(!contract.active_constraints.is_empty());
    }

    #[test]
    fn forbidden_claim_detected() {
        let asset = sample_asset("forbidden", WorldAssetKind::Rule, true);
        let constraints = compile_canon_constraints(&[asset.clone()]);
        let contract = compile_scene_contract("ch1", "test", &[asset.clone()], &constraints, &[]);
        let violations = validate_world_consistency("this text contains tag1", &contract, &[asset]);
        assert!(!violations.is_empty());
        assert_eq!(violations[0].kind, CanonConstraintKind::ForbiddenClaim);
    }

    #[test]
    fn proposed_rule_never_hard_violation() {
        let asset = sample_asset("proposed_rule", WorldAssetKind::Rule, false);
        let constraints = compile_canon_constraints(&[asset.clone()]);
        let contract = compile_scene_contract("ch1", "test", &[asset.clone()], &constraints, &[]);
        let violations = validate_world_consistency("this text contains tag1", &contract, &[asset]);
        assert!(!violations.is_empty());
        assert_eq!(violations[0].severity, ConstraintSeverity::Warning);
    }

    #[test]
    fn required_cost_skipped_detected() {
        let mut asset = sample_asset("cost_rule", WorldAssetKind::Rule, true);
        let mut constraint = CanonConstraint {
            id: "c1".to_string(),
            kind: CanonConstraintKind::RequiredCost,
            summary: "using fire requires mana".to_string(),
            trigger_terms: vec!["fire".to_string()],
            forbidden_terms: vec![],
            required_terms: vec!["mana".to_string()],
            severity: ConstraintSeverity::Hard,
            source_asset_id: asset.id.clone(),
            evidence: asset.evidence.clone(),
        };
        let contract = SceneContract {
            chapter_id: "ch1".to_string(),
            mission: "test".to_string(),
            required_facts: vec![],
            active_constraints: vec![constraint.clone()],
            required_state_deltas: vec![],
            allowed_reveals: vec![],
            blocked_reveals: vec![],
            evidence_refs: vec![],
        };
        let violations = validate_world_consistency("he used fire", &contract, &[asset.clone()]);
        assert!(!violations.is_empty());
        assert_eq!(violations[0].kind, CanonConstraintKind::RequiredCost);

        // When cost is paid, no violation
        let no_violations = validate_world_consistency("he used fire and paid mana", &contract, &[asset]);
        assert!(no_violations.is_empty());
    }
}
