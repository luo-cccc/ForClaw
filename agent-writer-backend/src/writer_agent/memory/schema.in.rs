const SCHEMA_VERSION: i64 = 20;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS canon_entities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    name TEXT NOT NULL UNIQUE,
    aliases_json TEXT DEFAULT '[]',
    summary TEXT DEFAULT '',
    attributes_json TEXT DEFAULT '{}',
    confidence REAL DEFAULT 0.5,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS canon_facts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    source_ref TEXT DEFAULT '',
    confidence REAL DEFAULT 0.5,
    status TEXT DEFAULT 'active',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS canon_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    source_ref TEXT DEFAULT '',
    status TEXT DEFAULT 'active',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS plot_promises (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT DEFAULT '',
    introduced_chapter TEXT DEFAULT '',
    introduced_ref TEXT DEFAULT '',
    last_seen_chapter TEXT DEFAULT '',
    last_seen_ref TEXT DEFAULT '',
    expected_payoff TEXT DEFAULT '',
    status TEXT DEFAULT 'open',
    priority INTEGER DEFAULT 0,
    risk_level TEXT DEFAULT 'medium',
    blocked_reason TEXT DEFAULT '',
    promoted INTEGER DEFAULT 0,
    core INTEGER DEFAULT 0,
    related_entities_json TEXT DEFAULT '[]',
    subject_ids_json TEXT DEFAULT '[]',
    subject_type TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS characters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    aliases_json TEXT DEFAULT '[]',
    role_type TEXT NOT NULL DEFAULT 'supporting',
    current_state_summary TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS character_state_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    character_id INTEGER NOT NULL,
    valid_from_chapter TEXT NOT NULL,
    valid_to_chapter TEXT DEFAULT '',
    core_commitments_json TEXT DEFAULT '[]',
    goal_state_json TEXT DEFAULT '{}',
    identity_state_json TEXT DEFAULT '{}',
    relationship_refs_json TEXT DEFAULT '[]',
    source_ref TEXT DEFAULT '',
    created_at INTEGER NOT NULL,
    FOREIGN KEY (character_id) REFERENCES characters(id)
);

CREATE TABLE IF NOT EXISTS character_relationships (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    character_a_id INTEGER NOT NULL,
    character_b_id INTEGER NOT NULL,
    relation_type TEXT NOT NULL DEFAULT 'neutral',
    visibility TEXT NOT NULL DEFAULT 'public',
    valid_from_chapter TEXT NOT NULL,
    valid_to_chapter TEXT DEFAULT '',
    source_ref TEXT DEFAULT '',
    FOREIGN KEY (character_a_id) REFERENCES characters(id),
    FOREIGN KEY (character_b_id) REFERENCES characters(id)
);

CREATE TABLE IF NOT EXISTS style_preferences (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL,
    evidence_ref TEXT DEFAULT '',
    confidence REAL DEFAULT 0.5,
    accepted_count INTEGER DEFAULT 0,
    rejected_count INTEGER DEFAULT 0,
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS creative_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope TEXT DEFAULT '',
    title TEXT NOT NULL,
    decision TEXT NOT NULL,
    alternatives_json TEXT DEFAULT '[]',
    rationale TEXT DEFAULT '',
    source_refs_json TEXT DEFAULT '[]',
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS proposal_feedback (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    proposal_id TEXT NOT NULL,
    action TEXT NOT NULL,
    final_text TEXT DEFAULT '',
    reason TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS story_contracts (
    project_id TEXT PRIMARY KEY,
    title TEXT DEFAULT '',
    genre TEXT DEFAULT '',
    target_reader TEXT DEFAULT '',
    reader_promise TEXT DEFAULT '',
    first_30_chapter_promise TEXT DEFAULT '',
    main_conflict TEXT DEFAULT '',
    structural_boundary TEXT DEFAULT '',
    tone_contract TEXT DEFAULT '',
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS chapter_missions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    chapter_title TEXT NOT NULL,
    mission TEXT DEFAULT '',
    must_include TEXT DEFAULT '',
    must_not TEXT DEFAULT '',
    expected_ending TEXT DEFAULT '',
    status TEXT DEFAULT 'draft',
    source_ref TEXT DEFAULT '',
    reader_lack_this_chapter TEXT DEFAULT '',
    relationship_soil_this_chapter TEXT DEFAULT '',
    pressure_scene TEXT DEFAULT '',
    interest_mechanism TEXT DEFAULT '',
    payoff_target TEXT DEFAULT '',
    payoff_path TEXT DEFAULT '',
    next_lack_opened TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now')),
    UNIQUE(project_id, chapter_title)
);

CREATE TABLE IF NOT EXISTS chapter_result_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    chapter_title TEXT NOT NULL,
    chapter_revision TEXT DEFAULT '',
    summary TEXT DEFAULT '',
    state_changes_json TEXT DEFAULT '[]',
    character_progress_json TEXT DEFAULT '[]',
    new_conflicts_json TEXT DEFAULT '[]',
    new_clues_json TEXT DEFAULT '[]',
    promise_updates_json TEXT DEFAULT '[]',
    canon_updates_json TEXT DEFAULT '[]',
    source_ref TEXT DEFAULT '',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    proposal_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    action TEXT NOT NULL,
    title TEXT NOT NULL,
    evidence TEXT DEFAULT '',
    rationale TEXT DEFAULT '',
    reason TEXT DEFAULT '',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_feedback_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    slot TEXT NOT NULL,
    category TEXT NOT NULL,
    action TEXT NOT NULL,
    confidence_delta REAL DEFAULT 0.0,
    source_error TEXT DEFAULT '',
    proposal_id TEXT DEFAULT '',
    reason TEXT DEFAULT '',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS writer_observation_trace (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    observation_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    reason TEXT NOT NULL,
    chapter_title TEXT DEFAULT '',
    paragraph_snippet TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS writer_proposal_trace (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    proposal_id TEXT NOT NULL UNIQUE,
    observation_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    priority TEXT NOT NULL,
    state TEXT NOT NULL,
    confidence REAL DEFAULT 0.0,
    preview_snippet TEXT DEFAULT '',
    evidence_json TEXT DEFAULT '[]',
    context_budget_json TEXT DEFAULT '',
    created_at INTEGER NOT NULL,
    expires_at INTEGER
);

CREATE TABLE IF NOT EXISTS writer_feedback_trace (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    proposal_id TEXT NOT NULL,
    action TEXT NOT NULL,
    reason TEXT DEFAULT '',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS writer_context_recalls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    source TEXT NOT NULL,
    reference TEXT NOT NULL,
    snippet TEXT DEFAULT '',
    recall_count INTEGER DEFAULT 0,
    first_recalled_at INTEGER NOT NULL,
    last_recalled_at INTEGER NOT NULL,
    last_observation_id TEXT DEFAULT '',
    last_proposal_id TEXT DEFAULT '',
    UNIQUE(project_id, source, reference)
);

CREATE TABLE IF NOT EXISTS writer_run_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    seq INTEGER NOT NULL,
    project_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    task_id TEXT DEFAULT '',
    event_type TEXT NOT NULL,
    source_refs_json TEXT DEFAULT '[]',
    data_json TEXT DEFAULT '{}',
    ts_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS manual_agent_turns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    observation_id TEXT DEFAULT '',
    chapter_title TEXT DEFAULT '',
    user_text TEXT NOT NULL,
    assistant_text TEXT NOT NULL,
    source_refs_json TEXT DEFAULT '[]',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS reader_compensation_profiles (
    project_id TEXT PRIMARY KEY,
    target_reader TEXT DEFAULT '',
    primary_lack TEXT DEFAULT '',
    secondary_lacks_json TEXT DEFAULT '[]',
    protagonist_proxy_state TEXT DEFAULT '',
    dominant_relationship_soil TEXT DEFAULT '',
    pressure_mode TEXT DEFAULT '',
    payoff_mode TEXT DEFAULT '',
    payoff_path TEXT DEFAULT '',
    escalation_ladder TEXT DEFAULT '',
    forbidden_shortcuts_json TEXT DEFAULT '[]',
    confidence REAL DEFAULT 0.5,
    source_refs_json TEXT DEFAULT '[]',
    pending_approval INTEGER DEFAULT 1,
    approved_by TEXT DEFAULT '',
    approved_at TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS emotional_debt_lifecycles (
    debt_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    debt_kind TEXT NOT NULL DEFAULT '',
    relationship_soil TEXT DEFAULT '',
    introduced_by_scene TEXT DEFAULT '',
    interest_mechanism TEXT DEFAULT '',
    payoff_contract TEXT DEFAULT '',
    payoff_window TEXT DEFAULT '',
    current_state TEXT DEFAULT 'introduced',
    overdue_risk TEXT DEFAULT 'medium',
    rollover_target TEXT DEFAULT '',
    source_refs_json TEXT DEFAULT '[]',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS emotional_debt_ledger (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    debt_kind TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT DEFAULT '',
    introduced_at TEXT DEFAULT '',
    introduced_chapter TEXT DEFAULT '',
    introduced_ref TEXT DEFAULT '',
    relationship_soil TEXT DEFAULT '',
    pressure_evidence TEXT DEFAULT '',
    interest_mechanism TEXT DEFAULT '',
    payoff_contract TEXT DEFAULT '',
    payoff_status TEXT DEFAULT 'open',
    expected_payoff_window TEXT DEFAULT '',
    payoff_path TEXT DEFAULT '',
    overdue_risk TEXT DEFAULT 'medium',
    rollover_target TEXT DEFAULT '',
    risk_level TEXT DEFAULT 'medium',
    related_promise_ids_json TEXT DEFAULT '[]',
    source_refs_json TEXT DEFAULT '[]',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS volumes (
    id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    title TEXT DEFAULT '',
    start_chapter INTEGER DEFAULT 0,
    end_chapter INTEGER DEFAULT 0,
    contract_json TEXT DEFAULT '{}',
    mission_json TEXT DEFAULT '{}',
    status TEXT DEFAULT 'active',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY(project_id, id)
);

CREATE TABLE IF NOT EXISTS volume_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    volume_id TEXT NOT NULL,
    snapshot_json TEXT DEFAULT '{}',
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS arc_snapshots (
    arc_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    volume_id TEXT NOT NULL,
    title TEXT DEFAULT '',
    start_chapter INTEGER DEFAULT 0,
    end_chapter INTEGER DEFAULT 0,
    snapshot_json TEXT DEFAULT '{}',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY(project_id, arc_id)
);

CREATE TABLE IF NOT EXISTS book_state (
    project_id TEXT PRIMARY KEY,
    title TEXT DEFAULT '',
    long_term_constraints_json TEXT DEFAULT '[]',
    mega_promises_json TEXT DEFAULT '[]',
    irreversible_changes_json TEXT DEFAULT '[]',
    source_ref TEXT DEFAULT '',
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS supervised_sprints (
    project_id TEXT NOT NULL,
    sprint_id TEXT NOT NULL,
    status TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    last_checkpoint_id TEXT DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(project_id, sprint_id)
);

CREATE TABLE IF NOT EXISTS supervised_sprint_checkpoints (
    project_id TEXT NOT NULL,
    checkpoint_id TEXT NOT NULL,
    sprint_id TEXT NOT NULL,
    checkpoint_json TEXT NOT NULL,
    source TEXT DEFAULT '',
    created_at INTEGER NOT NULL,
    PRIMARY KEY(project_id, checkpoint_id)
);

CREATE TABLE IF NOT EXISTS knowledge_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic TEXT NOT NULL UNIQUE,
    truth_state TEXT NOT NULL DEFAULT 'objective',
    source_ref TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS knowledge_ownership (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    knowledge_id INTEGER NOT NULL,
    holder_type TEXT NOT NULL,
    holder_id INTEGER NOT NULL,
    knowledge_mode TEXT NOT NULL DEFAULT 'aware',
    valid_from_chapter TEXT NOT NULL,
    valid_to_chapter TEXT DEFAULT '',
    source_ref TEXT DEFAULT '',
    FOREIGN KEY (knowledge_id) REFERENCES knowledge_items(id)
);

CREATE TABLE IF NOT EXISTS identity_layers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    character_id INTEGER NOT NULL,
    public_identity TEXT DEFAULT '',
    private_identity TEXT DEFAULT '',
    revealed_to_json TEXT DEFAULT '[]',
    valid_from_chapter TEXT NOT NULL,
    valid_to_chapter TEXT DEFAULT '',
    FOREIGN KEY (character_id) REFERENCES characters(id)
);

CREATE TABLE IF NOT EXISTS reveal_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject_id INTEGER NOT NULL,
    reveal_type TEXT NOT NULL,
    revealed_to TEXT NOT NULL,
    chapter TEXT NOT NULL,
    source_ref TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS scenes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chapter_title TEXT NOT NULL,
    sequence INTEGER NOT NULL DEFAULT 0,
    scene_type TEXT DEFAULT 'scene',
    summary TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS scene_state (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scene_id INTEGER NOT NULL,
    objective TEXT DEFAULT '',
    participants_json TEXT DEFAULT '[]',
    location_ref TEXT DEFAULT '',
    entry_state_json TEXT DEFAULT '{}',
    exit_state_json TEXT DEFAULT '{}',
    FOREIGN KEY (scene_id) REFERENCES scenes(id)
);

CREATE TABLE IF NOT EXISTS scene_obligations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scene_id INTEGER NOT NULL,
    promise_ids_json TEXT DEFAULT '[]',
    mission_refs_json TEXT DEFAULT '[]',
    payoff_targets_json TEXT DEFAULT '[]',
    FOREIGN KEY (scene_id) REFERENCES scenes(id)
);

CREATE TABLE IF NOT EXISTS scene_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scene_id INTEGER NOT NULL,
    outcome TEXT DEFAULT '',
    consequence TEXT DEFAULT '',
    source_ref TEXT DEFAULT '',
    FOREIGN KEY (scene_id) REFERENCES scenes(id)
);

CREATE TABLE IF NOT EXISTS story_time_slices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    label TEXT NOT NULL,
    relative_order INTEGER DEFAULT 0,
    start_ref TEXT DEFAULT '',
    end_ref TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS chapter_time_mapping (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chapter_title TEXT NOT NULL,
    scene_id INTEGER,
    time_slice_id INTEGER NOT NULL,
    narrative_mode TEXT DEFAULT 'present',
    FOREIGN KEY (time_slice_id) REFERENCES story_time_slices(id)
);

CREATE TABLE IF NOT EXISTS timeline_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject_ids_json TEXT DEFAULT '[]',
    event_type TEXT NOT NULL,
    time_slice_id INTEGER NOT NULL,
    source_ref TEXT DEFAULT '',
    FOREIGN KEY (time_slice_id) REFERENCES story_time_slices(id)
);
"#;

const INDEX_SCHEMA: &str = r#"
CREATE INDEX IF NOT EXISTS idx_canon_name ON canon_entities(name);
CREATE INDEX IF NOT EXISTS idx_canon_kind ON canon_entities(kind);
CREATE INDEX IF NOT EXISTS idx_canon_fact_entity ON canon_facts(entity_id);
CREATE INDEX IF NOT EXISTS idx_canon_fact_key ON canon_facts(key);
CREATE INDEX IF NOT EXISTS idx_canon_rule_category ON canon_rules(category);
CREATE INDEX IF NOT EXISTS idx_canon_rule_status ON canon_rules(status);
CREATE INDEX IF NOT EXISTS idx_promise_status ON plot_promises(status);
CREATE INDEX IF NOT EXISTS idx_feedback_proposal ON proposal_feedback(proposal_id);
CREATE INDEX IF NOT EXISTS idx_story_contract_updated_at ON story_contracts(updated_at);
CREATE INDEX IF NOT EXISTS idx_chapter_missions_project_chapter ON chapter_missions(project_id, chapter_title);
CREATE INDEX IF NOT EXISTS idx_chapter_missions_status ON chapter_missions(status);
CREATE INDEX IF NOT EXISTS idx_chapter_result_project_created ON chapter_result_snapshots(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_chapter_result_project_chapter ON chapter_result_snapshots(project_id, chapter_title);
CREATE INDEX IF NOT EXISTS idx_memory_audit_created_at ON memory_audit_events(created_at);
CREATE INDEX IF NOT EXISTS idx_memory_feedback_created_at ON memory_feedback_events(created_at);
CREATE INDEX IF NOT EXISTS idx_memory_feedback_slot_created ON memory_feedback_events(slot, created_at);
CREATE INDEX IF NOT EXISTS idx_observation_trace_created_at ON writer_observation_trace(created_at);
CREATE INDEX IF NOT EXISTS idx_proposal_trace_created_at ON writer_proposal_trace(created_at);
CREATE INDEX IF NOT EXISTS idx_proposal_trace_proposal_id ON writer_proposal_trace(proposal_id);
CREATE INDEX IF NOT EXISTS idx_feedback_trace_created_at ON writer_feedback_trace(created_at);
CREATE INDEX IF NOT EXISTS idx_context_recalls_project_last ON writer_context_recalls(project_id, last_recalled_at);
CREATE INDEX IF NOT EXISTS idx_context_recalls_project_count ON writer_context_recalls(project_id, recall_count);
CREATE INDEX IF NOT EXISTS idx_writer_run_events_project_session_seq ON writer_run_events(project_id, session_id, seq);
CREATE INDEX IF NOT EXISTS idx_writer_run_events_project_ts ON writer_run_events(project_id, ts_ms);
CREATE INDEX IF NOT EXISTS idx_manual_agent_turns_project_created_at ON manual_agent_turns(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_char_state_character_chapter ON character_state_versions(character_id, valid_from_chapter);
CREATE INDEX IF NOT EXISTS idx_char_rel_char_a ON character_relationships(character_a_id, valid_from_chapter);
CREATE INDEX IF NOT EXISTS idx_char_rel_char_b ON character_relationships(character_b_id, valid_from_chapter);
CREATE INDEX IF NOT EXISTS idx_knowledge_ownership_holder ON knowledge_ownership(holder_type, holder_id, valid_from_chapter);
CREATE INDEX IF NOT EXISTS idx_identity_layers_character ON identity_layers(character_id, valid_from_chapter);
CREATE INDEX IF NOT EXISTS idx_scenes_chapter ON scenes(chapter_title);
CREATE INDEX IF NOT EXISTS idx_chapter_time_mapping_chapter ON chapter_time_mapping(chapter_title);

CREATE INDEX IF NOT EXISTS idx_rcp_project ON reader_compensation_profiles(project_id);
CREATE INDEX IF NOT EXISTS idx_edl_project_state ON emotional_debt_lifecycles(project_id, current_state);
CREATE INDEX IF NOT EXISTS idx_edl_kind ON emotional_debt_lifecycles(debt_kind);
CREATE INDEX IF NOT EXISTS idx_edlgr_project_status ON emotional_debt_ledger(project_id, payoff_status);
CREATE INDEX IF NOT EXISTS idx_edlgr_kind_risk ON emotional_debt_ledger(debt_kind, overdue_risk);
CREATE INDEX IF NOT EXISTS idx_edlgr_introduced ON emotional_debt_ledger(introduced_chapter);
CREATE INDEX IF NOT EXISTS idx_volumes_project_range ON volumes(project_id, start_chapter, end_chapter);
CREATE INDEX IF NOT EXISTS idx_volume_snapshots_project_volume ON volume_snapshots(project_id, volume_id);
CREATE INDEX IF NOT EXISTS idx_arc_snapshots_project_volume ON arc_snapshots(project_id, volume_id);
CREATE INDEX IF NOT EXISTS idx_supervised_sprints_project_status ON supervised_sprints(project_id, status, updated_at);
CREATE INDEX IF NOT EXISTS idx_supervised_sprint_checkpoints_sprint ON supervised_sprint_checkpoints(project_id, sprint_id, created_at);
"#;

const SCHEMA_V20_ALTERS: &str = r#"
ALTER TABLE character_state_versions ADD COLUMN time_slice_id INTEGER;
ALTER TABLE character_relationships ADD COLUMN time_slice_id INTEGER;
ALTER TABLE identity_layers ADD COLUMN time_slice_id INTEGER;
"#;
