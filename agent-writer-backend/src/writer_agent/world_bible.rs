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
    /// 约束适用的实体/范围（如特定角色、势力、地点）
    pub applies_to: Vec<String>,
    /// 违反约束的预期后果（用于报告建议）
    pub expected_consequence: String,
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
    /// 连续性锚点（从前章继承的必须保持的状态）
    pub continuity_anchors: Vec<String>,
    /// 本章必须支付的代价（从 RequiredCost 约束编译）
    pub required_costs: Vec<String>,
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

// ── P18: Preflight Checks ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightWarning {
    pub code: String,
    pub severity: ConstraintSeverity,
    pub message: String,
    pub source_asset_id: String,
    pub suggested_action: String,
}

/// Run preflight checks before chapter generation.
/// Detects proposed assets masquerading as hard constraints,
/// missing evidence, and other world-bible readiness issues.
pub fn preflight_world_bible(
    assets: &[WorldAsset],
    constraints: &[CanonConstraint],
) -> Vec<PreflightWarning> {
    let mut warnings = Vec::new();

    for constraint in constraints {
        let source = assets.iter().find(|a| a.id == constraint.source_asset_id);

        // Check 1: proposed asset used as hard constraint
        if let Some(asset) = source {
            if !asset.can_hard_enforce() && constraint.severity == ConstraintSeverity::Hard {
                warnings.push(PreflightWarning {
                    code: "proposed_as_hard".into(),
                    severity: ConstraintSeverity::Warning,
                    message: format!(
                        "Asset '{}' is {} but constraint '{}' is marked Hard. It will be downgraded to Warning.",
                        asset.id,
                        serde_json::to_string(&asset.approval_status).unwrap_or_default().trim_matches('"'),
                        constraint.id
                    ),
                    source_asset_id: asset.id.clone(),
                    suggested_action: "approve_asset_or_downgrade_constraint".into(),
                });
            }
        }

        // Check 2: no evidence on hard constraint source
        if constraint.severity == ConstraintSeverity::Hard
            && source.map(|a| !a.has_evidence()).unwrap_or(true)
        {
            warnings.push(PreflightWarning {
                code: "hard_without_evidence".into(),
                severity: ConstraintSeverity::Warning,
                message: format!(
                    "Hard constraint '{}' lacks evidence on its source asset.",
                    constraint.id
                ),
                source_asset_id: constraint.source_asset_id.clone(),
                suggested_action: "add_evidence_or_downgrade_to_warning".into(),
            });
        }

        // Check 3: empty trigger/required terms on actionable constraint kinds
        match constraint.kind {
            CanonConstraintKind::ForbiddenClaim if constraint.forbidden_terms.is_empty() => {
                warnings.push(PreflightWarning {
                    code: "empty_forbidden_terms".into(),
                    severity: ConstraintSeverity::Info,
                    message: format!(
                        "ForbiddenClaim constraint '{}' has no forbidden_terms and will never trigger.",
                        constraint.id
                    ),
                    source_asset_id: constraint.source_asset_id.clone(),
                    suggested_action: "add_forbidden_terms".into(),
                });
            }
            CanonConstraintKind::RequiredCost
                if constraint.trigger_terms.is_empty() || constraint.required_terms.is_empty() =>
            {
                warnings.push(PreflightWarning {
                    code: "empty_cost_terms".into(),
                    severity: ConstraintSeverity::Info,
                    message: format!(
                        "RequiredCost constraint '{}' is missing trigger or required terms.",
                        constraint.id
                    ),
                    source_asset_id: constraint.source_asset_id.clone(),
                    suggested_action: "add_trigger_and_required_terms".into(),
                });
            }
            _ => {}
        }
    }

    warnings
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
    for asset in assets
        .iter()
        .filter(|a| matches!(a.kind, WorldAssetKind::Rule))
    {
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
            applies_to: Vec::new(),
            expected_consequence: String::new(),
        });
    }
    constraints
}

/// Compile a SceneContract from mission, world assets, and pre-built constraints.
/// Selects the most relevant constraints based on mission keywords.
/// `max_constraints` defaults to 8 if not provided.
pub fn compile_scene_contract(
    chapter_id: &str,
    mission: &str,
    assets: &[WorldAsset],
    constraints: &[CanonConstraint],
    required_deltas: &[crate::chapter_generation::StateDelta],
    max_constraints: Option<usize>,
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
        b_score
            .partial_cmp(&a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let selected: Vec<CanonConstraint> = scored
        .iter()
        .take(max_constraints.unwrap_or(8))
        .map(|(_, c)| (*c).clone())
        .collect();

    let evidence_refs: Vec<EvidenceRef> =
        selected.iter().flat_map(|c| c.evidence.clone()).collect();

    SceneContract {
        chapter_id: chapter_id.to_string(),
        mission: mission.to_string(),
        required_facts: Vec::new(),
        active_constraints: selected,
        required_state_deltas: required_deltas.to_vec(),
        allowed_reveals: Vec::new(),
        blocked_reveals: Vec::new(),
        evidence_refs,
        continuity_anchors: Vec::new(),
        required_costs: Vec::new(),
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
                                suggested_fix: format!("在触发 '{}' 时确保支付代价", trigger),
                            });
                        }
                    }
                }
            }
            CanonConstraintKind::HierarchyLimit => {
                // Simplified: if both a low-tier identity and a high-tier action appear, warn
                let low_tier = constraint
                    .trigger_terms
                    .iter()
                    .any(|t| text_lower.contains(&t.to_lowercase()));
                let high_action = constraint
                    .forbidden_terms
                    .iter()
                    .any(|t| text_lower.contains(&t.to_lowercase()));
                if low_tier && high_action {
                    violations.push(WorldConsistencyViolation {
                        constraint_id: constraint.id.clone(),
                        severity: effective.clone(),
                        kind: CanonConstraintKind::HierarchyLimit,
                        message: format!(
                            "层级限制可能被突破: {:?} 尝试 {:?}",
                            constraint.trigger_terms, constraint.forbidden_terms
                        ),
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

// ── P19: Story State Ledger ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateLedgerEntry {
    pub chapter_id: String,
    pub timestamp_ms: u64,
    pub deltas: Vec<StateLedgerDelta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateLedgerDelta {
    pub delta_type: String,
    pub entity_id: String,
    pub before_state: String,
    pub after_state: String,
    pub source_constraint_id: Option<String>,
    pub evidence_excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateLedger {
    pub project_id: String,
    pub entries: Vec<StateLedgerEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateLedgerRegression {
    pub delta_type: String,
    pub entity_id: String,
    pub prior_after_state: String,
    pub current_observed_state: String,
    pub message: String,
}

/// Extract state changes from chapter text by checking if required_state_deltas are covered
/// and whether any active constraints were triggered (e.g., RequiredCost was paid).
pub fn extract_state_deltas_from_chapter(
    chapter_text: &str,
    contract: &SceneContract,
    _assets: &[WorldAsset],
) -> Vec<StateLedgerDelta> {
    let mut deltas = Vec::new();
    let text_lower = chapter_text.to_lowercase();

    // Check required_state_deltas from the contract: heuristic keyword match on description
    for req in &contract.required_state_deltas {
        let desc_lower = req.description.to_lowercase();
        // Simple heuristic: if the description contains a "->" separator, treat as before->after
        if let Some(arrow_pos) = desc_lower.find("->") {
            let before = req.description[..arrow_pos].trim().to_string();
            let after = req.description[arrow_pos + 2..].trim().to_string();
            let before_lower = before.to_lowercase();
            let after_lower = after.to_lowercase();

            // Determine if the text mentions the after-state (covered) or still the before-state
            let after_mentioned = text_lower.contains(&after_lower);
            let before_mentioned = text_lower.contains(&before_lower);

            if after_mentioned || before_mentioned {
                deltas.push(StateLedgerDelta {
                    delta_type: req.delta_type.clone(),
                    entity_id: req.source.clone(),
                    before_state: before.clone(),
                    after_state: after.clone(),
                    source_constraint_id: None,
                    evidence_excerpt: extract_excerpt(chapter_text, &after),
                });
            }
        } else {
            // No arrow: treat the whole description as the after-state and look for it
            let term = req.description.clone();
            let term_lower = term.to_lowercase();
            if text_lower.contains(&term_lower) {
                deltas.push(StateLedgerDelta {
                    delta_type: req.delta_type.clone(),
                    entity_id: req.source.clone(),
                    before_state: String::new(),
                    after_state: term.clone(),
                    source_constraint_id: None,
                    evidence_excerpt: extract_excerpt(chapter_text, &term),
                });
            }
        }
    }

    // Check active constraints for triggered RequiredCost (treated as a state delta)
    for constraint in &contract.active_constraints {
        if constraint.kind != CanonConstraintKind::RequiredCost {
            continue;
        }
        for trigger in &constraint.trigger_terms {
            let trigger_lower = trigger.to_lowercase();
            if text_lower.contains(&trigger_lower) {
                let cost_paid = constraint
                    .required_terms
                    .iter()
                    .any(|req| text_lower.contains(&req.to_lowercase()));
                deltas.push(StateLedgerDelta {
                    delta_type: "rule_triggered".to_string(),
                    entity_id: constraint.id.clone(),
                    before_state: "unpaid".to_string(),
                    after_state: if cost_paid {
                        "paid".to_string()
                    } else {
                        "unpaid".to_string()
                    },
                    source_constraint_id: Some(constraint.id.clone()),
                    evidence_excerpt: extract_excerpt(chapter_text, trigger),
                });
            }
        }
    }

    deltas
}

/// Check if current chapter text contradicts prior state changes without explanation.
/// A regression is when a prior "after_state" appears as "before_state" again
/// without a transition marker (heuristic: no "->" or "changed" or "reverted" nearby).
pub fn check_state_regression(
    current_text: &str,
    prior_deltas: &[StateLedgerDelta],
) -> Vec<StateLedgerRegression> {
    let mut regressions = Vec::new();
    let text_lower = current_text.to_lowercase();

    for delta in prior_deltas {
        if delta.after_state.is_empty() {
            continue;
        }
        let after_lower = delta.after_state.to_lowercase();
        let before_lower = delta.before_state.to_lowercase();

        // Regression: text shows the before-state again, contradicting the after-state
        if !before_lower.is_empty() && text_lower.contains(&before_lower) {
            // Check if there's an explicit transition marker nearby (simple heuristic)
            let has_transition_marker = text_lower.contains("->")
                || text_lower.contains("changed")
                || text_lower.contains("reverted")
                || text_lower.contains("returned")
                || text_lower.contains("back to");

            if !has_transition_marker {
                regressions.push(StateLedgerRegression {
                    delta_type: delta.delta_type.clone(),
                    entity_id: delta.entity_id.clone(),
                    prior_after_state: delta.after_state.clone(),
                    current_observed_state: delta.before_state.clone(),
                    message: format!(
                        "Entity '{}' regressed from '{}' to '{}' without explanation",
                        delta.entity_id, delta.after_state, delta.before_state
                    ),
                });
            }
        }

        // Also flag if the after-state is explicitly negated
        let negation_phrases = [
            format!("no longer {}", after_lower),
            format!("not {} anymore", after_lower),
            format!("{} was false", after_lower),
        ];
        for phrase in &negation_phrases {
            if text_lower.contains(phrase) {
                regressions.push(StateLedgerRegression {
                    delta_type: delta.delta_type.clone(),
                    entity_id: delta.entity_id.clone(),
                    prior_after_state: delta.after_state.clone(),
                    current_observed_state: format!("negated: {}", delta.after_state),
                    message: format!(
                        "Entity '{}' prior state '{}' was explicitly negated",
                        delta.entity_id, delta.after_state
                    ),
                });
                break;
            }
        }
    }

    regressions
}

/// Compile a list of forbidden state descriptions from prior deltas.
/// Used by SceneContract.blocked_reveals or similar.
pub fn compile_forbidden_regressions(prior_deltas: &[StateLedgerDelta]) -> Vec<String> {
    prior_deltas
        .iter()
        .filter(|d| !d.after_state.is_empty())
        .map(|d| {
            format!(
                "{}: {} must not revert to '{}' (was advanced to '{}')",
                d.delta_type, d.entity_id, d.before_state, d.after_state
            )
        })
        .collect()
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
        let contract =
            compile_scene_contract("ch1", "fire scene", &assets, &constraints, &[], None);
        // fire_rule should be selected because its trigger term "fire_rule" loosely matches "fire scene"
        assert!(!contract.active_constraints.is_empty());
    }

    #[test]
    fn forbidden_claim_detected() {
        let asset = sample_asset("forbidden", WorldAssetKind::Rule, true);
        let constraints = compile_canon_constraints(std::slice::from_ref(&asset));
        let contract =
            compile_scene_contract("ch1", "test", std::slice::from_ref(&asset), &constraints, &[], None);
        let violations = validate_world_consistency("this text contains tag1", &contract, &[asset]);
        assert!(!violations.is_empty());
        assert_eq!(violations[0].kind, CanonConstraintKind::ForbiddenClaim);
    }

    #[test]
    fn proposed_rule_never_hard_violation() {
        let asset = sample_asset("proposed_rule", WorldAssetKind::Rule, false);
        let constraints = compile_canon_constraints(std::slice::from_ref(&asset));
        let contract =
            compile_scene_contract("ch1", "test", std::slice::from_ref(&asset), &constraints, &[], None);
        let violations = validate_world_consistency("this text contains tag1", &contract, &[asset]);
        assert!(!violations.is_empty());
        assert_eq!(violations[0].severity, ConstraintSeverity::Warning);
    }

    #[test]
    fn required_cost_skipped_detected() {
        let asset = sample_asset("cost_rule", WorldAssetKind::Rule, true);
        let constraint = CanonConstraint {
            id: "c1".to_string(),
            kind: CanonConstraintKind::RequiredCost,
            summary: "using fire requires mana".to_string(),
            trigger_terms: vec!["fire".to_string()],
            forbidden_terms: vec![],
            required_terms: vec!["mana".to_string()],
            severity: ConstraintSeverity::Hard,
            source_asset_id: asset.id.clone(),
            evidence: asset.evidence.clone(),
            applies_to: Vec::new(),
            expected_consequence: String::new(),
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
            continuity_anchors: Vec::new(),
            required_costs: Vec::new(),
        };
        let violations = validate_world_consistency("he used fire", &contract, std::slice::from_ref(&asset));
        assert!(!violations.is_empty());
        assert_eq!(violations[0].kind, CanonConstraintKind::RequiredCost);

        // When cost is paid, no violation
        let no_violations =
            validate_world_consistency("he used fire and paid mana", &contract, &[asset]);
        assert!(no_violations.is_empty());
    }

    #[test]
    fn hierarchy_limit_detected_when_low_tier_high_action() {
        let asset = sample_asset("hierarchy_rule", WorldAssetKind::Rule, true);
        let constraint = CanonConstraint {
            id: "c1".to_string(),
            kind: CanonConstraintKind::HierarchyLimit,
            summary: "炼气期不可使用金丹期法宝".to_string(),
            trigger_terms: vec!["炼气".to_string()],
            forbidden_terms: vec!["金丹法宝".to_string()],
            required_terms: vec![],
            severity: ConstraintSeverity::Hard,
            source_asset_id: asset.id.clone(),
            evidence: asset.evidence.clone(),
            applies_to: Vec::new(),
            expected_consequence: String::new(),
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
            continuity_anchors: Vec::new(),
            required_costs: Vec::new(),
        };
        let violations = validate_world_consistency(
            "他只是一个炼气期弟子，却催动了金丹法宝",
            &contract,
            &[asset],
        );
        assert!(!violations.is_empty());
        assert_eq!(violations[0].kind, CanonConstraintKind::HierarchyLimit);
    }

    #[test]
    fn violation_includes_source_evidence() {
        let asset = sample_asset("forbidden", WorldAssetKind::Rule, true);
        let constraints = compile_canon_constraints(std::slice::from_ref(&asset));
        let contract =
            compile_scene_contract("ch1", "test", std::slice::from_ref(&asset), &constraints, &[], None);
        let violations =
            validate_world_consistency("this text contains tag1", &contract, std::slice::from_ref(&asset));
        assert!(!violations.is_empty());
        // Evidence should be preserved from constraint -> violation
        assert!(
            !violations[0].evidence.is_empty(),
            "violation should carry source evidence"
        );
        assert_eq!(violations[0].evidence[0].source_id, "src1");
        assert_eq!(violations[0].evidence[0].excerpt, "evidence");
    }

    // ── P18 Preflight Tests ──

    #[test]
    fn preflight_catches_proposed_as_hard() {
        let proposed = sample_asset("proposed_rule", WorldAssetKind::Rule, false);
        // Manually construct a Hard constraint from a proposed asset to test preflight logic
        let constraints = vec![CanonConstraint {
            id: "c1".to_string(),
            kind: CanonConstraintKind::ForbiddenClaim,
            summary: "test".to_string(),
            trigger_terms: vec!["fire".to_string()],
            forbidden_terms: vec!["water".to_string()],
            required_terms: vec![],
            severity: ConstraintSeverity::Hard,
            source_asset_id: proposed.id.clone(),
            evidence: proposed.evidence.clone(),
            applies_to: Vec::new(),
            expected_consequence: String::new(),
        }];
        let warnings = preflight_world_bible(std::slice::from_ref(&proposed), &constraints);
        assert!(
            warnings.iter().any(|w| w.code == "proposed_as_hard"),
            "preflight should warn when proposed asset compiles to hard constraint"
        );
    }

    #[test]
    fn preflight_catches_hard_without_evidence() {
        let mut asset = sample_asset("no_evidence", WorldAssetKind::Rule, true);
        asset.evidence.clear(); // Remove evidence
                                // Manually construct a Hard constraint to test preflight logic
        let constraints = vec![CanonConstraint {
            id: "c1".to_string(),
            kind: CanonConstraintKind::ForbiddenClaim,
            summary: "test".to_string(),
            trigger_terms: vec!["fire".to_string()],
            forbidden_terms: vec!["water".to_string()],
            required_terms: vec![],
            severity: ConstraintSeverity::Hard,
            source_asset_id: asset.id.clone(),
            evidence: vec![],
            applies_to: Vec::new(),
            expected_consequence: String::new(),
        }];
        let warnings = preflight_world_bible(&[asset.clone()], &constraints);
        assert!(
            warnings.iter().any(|w| w.code == "hard_without_evidence"),
            "preflight should warn when hard constraint lacks evidence"
        );
    }

    #[test]
    fn preflight_allows_approved_with_evidence() {
        let asset = sample_asset("approved_rule", WorldAssetKind::Rule, true);
        let constraints = compile_canon_constraints(std::slice::from_ref(&asset));
        let warnings = preflight_world_bible(std::slice::from_ref(&asset), &constraints);
        assert!(
            warnings.is_empty(),
            "preflight should have no warnings for fully approved, evidenced rule"
        );
    }

    #[test]
    fn preflight_warns_empty_forbidden_terms() {
        let mut asset = sample_asset("empty_rule", WorldAssetKind::Rule, true);
        asset.tags.clear(); // This makes forbidden_terms empty in compiled constraint
        let constraints = compile_canon_constraints(std::slice::from_ref(&asset));
        let warnings = preflight_world_bible(std::slice::from_ref(&asset), &constraints);
        assert!(
            warnings.iter().any(|w| w.code == "empty_forbidden_terms"),
            "preflight should warn about empty forbidden terms"
        );
    }

    // ── P19 Story State Ledger Tests ──

    #[test]
    fn state_delta_extraction_finds_covered_delta() {
        let contract = SceneContract {
            chapter_id: "ch2".to_string(),
            mission: "test".to_string(),
            required_facts: vec![],
            active_constraints: vec![],
            required_state_deltas: vec![crate::chapter_generation::StateDelta {
                delta_type: "character_knowledge".to_string(),
                description: "ignorant -> knows the truth".to_string(),
                source: "hero".to_string(),
            }],
            allowed_reveals: vec![],
            blocked_reveals: vec![],
            evidence_refs: vec![],
            continuity_anchors: Vec::new(),
            required_costs: Vec::new(),
        };
        let deltas = extract_state_deltas_from_chapter(
            "The hero finally knows the truth about his past.",
            &contract,
            &[],
        );
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].delta_type, "character_knowledge");
        assert_eq!(deltas[0].entity_id, "hero");
        assert_eq!(deltas[0].before_state, "ignorant");
        assert_eq!(deltas[0].after_state, "knows the truth");
    }

    #[test]
    fn state_regression_detects_unexplained_rollback() {
        let prior_deltas = vec![StateLedgerDelta {
            delta_type: "relationship".to_string(),
            entity_id: "alice_bob".to_string(),
            before_state: "strangers".to_string(),
            after_state: "friends".to_string(),
            source_constraint_id: None,
            evidence_excerpt: "they shook hands".to_string(),
        }];
        // Current text reverts to the before-state without any transition marker
        let regressions =
            check_state_regression("Alice and Bob were strangers again.", &prior_deltas);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].entity_id, "alice_bob");
        assert_eq!(regressions[0].prior_after_state, "friends");
        assert_eq!(regressions[0].current_observed_state, "strangers");
        assert!(
            regressions[0].message.contains("regressed"),
            "message should mention regression"
        );
    }

    #[test]
    fn forbidden_regressions_compiled_from_prior_deltas() {
        let prior_deltas = vec![
            StateLedgerDelta {
                delta_type: "resource".to_string(),
                entity_id: "gold_coins".to_string(),
                before_state: "100".to_string(),
                after_state: "50".to_string(),
                source_constraint_id: None,
                evidence_excerpt: "spent 50 coins".to_string(),
            },
            StateLedgerDelta {
                delta_type: "character_knowledge".to_string(),
                entity_id: "villain".to_string(),
                before_state: "unknown".to_string(),
                after_state: "revealed".to_string(),
                source_constraint_id: None,
                evidence_excerpt: "the mask fell".to_string(),
            },
        ];
        let forbidden = compile_forbidden_regressions(&prior_deltas);
        assert_eq!(forbidden.len(), 2);
        assert!(forbidden[0].contains("gold_coins"));
        assert!(forbidden[0].contains("must not revert to '100'"));
        assert!(forbidden[1].contains("villain"));
        assert!(forbidden[1].contains("must not revert to 'unknown'"));
    }
}
