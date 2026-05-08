const HERMES_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftProfile {
    pub id: i64,
    pub key: String,
    pub value: String,
    pub confidence: f64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkill {
    pub id: i64,
    pub skill: String,
    pub category: String,
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionSearchResult {
    pub role: String,
    pub content: String,
    pub created_at: String,
}

pub struct HermesDB {
    conn: Connection,
}
