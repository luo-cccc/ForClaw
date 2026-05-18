use rusqlite::{Connection, OpenFlags, Row};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalWritingDbStatus {
    pub configured: bool,
    pub db_path: Option<String>,
    pub readable: bool,
    pub table_count: usize,
    pub view_count: usize,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalWritingTaskQuery {
    pub task_code: String,
    pub task_name: String,
    pub problem_statement: String,
    pub situation_codes: String,
    pub workflow_stage: String,
    pub first_action: String,
    pub stop_condition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalWritingRuleAsset {
    pub situation_code: Option<String>,
    pub workflow_stage: Option<String>,
    pub rule_type: String,
    pub rule_code: String,
    pub rule_name: String,
    pub rule_summary: String,
    pub usage_note: String,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalWritingReference {
    pub reference_id: i64,
    pub situation: String,
    pub novel: String,
    pub chapter_no: i64,
    pub chapter: String,
    pub why_selected: String,
    pub reading_task: String,
    pub summary: String,
    pub file_path: String,
    pub scene_goal: Option<String>,
    pub opposition: Option<String>,
    pub leverage: Option<String>,
    pub reversal: Option<String>,
    pub cost: Option<String>,
    pub aftereffect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalReferenceRule {
    pub reference_id: i64,
    pub situation: String,
    pub novel: String,
    pub chapter_no: i64,
    pub chapter: String,
    pub rule_type: String,
    pub rule_code: String,
    pub rule_name: String,
    pub rule_summary: String,
    pub relevance: i64,
    pub usage_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalWritingContextBundle {
    pub query: String,
    pub task_queries: Vec<ExternalWritingTaskQuery>,
    pub situation_rules: Vec<ExternalWritingRuleAsset>,
    pub workflow_rules: Vec<ExternalWritingRuleAsset>,
    pub references: Vec<ExternalWritingReference>,
    pub reference_rules: Vec<ExternalReferenceRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEvalSignal {
    pub code: String,
    pub failure_kind: String,
    pub description: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalPromptTemplate {
    pub code: String,
    pub title: String,
    pub workflow_stage: String,
    pub summary: String,
}

fn db_path_from_env() -> Option<PathBuf> {
    std::env::var("FORGE_EXTERNAL_WRITING_DB")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

pub fn configured_db_path() -> Option<PathBuf> {
    db_path_from_env()
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Failed to open external writing DB: {}", e))
}

pub fn external_writing_db_status() -> ExternalWritingDbStatus {
    let Some(path) = configured_db_path() else {
        return ExternalWritingDbStatus {
            configured: false,
            db_path: None,
            readable: false,
            table_count: 0,
            view_count: 0,
            notes: vec!["FORGE_EXTERNAL_WRITING_DB is not configured.".to_string()],
        };
    };

    let mut notes = Vec::new();
    let db_path = Some(path.to_string_lossy().to_string());
    if !path.exists() {
        notes.push("Configured database path does not exist.".to_string());
        return ExternalWritingDbStatus {
            configured: true,
            db_path,
            readable: false,
            table_count: 0,
            view_count: 0,
            notes,
        };
    }

    match open_read_only(&path) {
        Ok(conn) => {
            let table_count = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize;
            let view_count = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='view'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize;
            notes.push("External writing database is readable.".to_string());
            ExternalWritingDbStatus {
                configured: true,
                db_path,
                readable: true,
                table_count,
                view_count,
                notes,
            }
        }
        Err(error) => ExternalWritingDbStatus {
            configured: true,
            db_path,
            readable: false,
            table_count: 0,
            view_count: 0,
            notes: vec![error],
        },
    }
}

fn query_map_vec<T, F>(conn: &Connection, sql: &str, mapper: F) -> Result<Vec<T>, String>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("Failed to prepare external DB query: {}", e))?;
    let rows = stmt
        .query_map([], mapper)
        .map_err(|e| format!("Failed to execute external DB query: {}", e))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| format!("Failed to map external DB query results: {}", e))
}

pub fn load_task_queries(limit: usize) -> Result<Vec<ExternalWritingTaskQuery>, String> {
    let path = configured_db_path()
        .ok_or_else(|| "FORGE_EXTERNAL_WRITING_DB is not configured.".to_string())?;
    let conn = open_read_only(&path)?;
    query_map_vec(
        &conn,
        &format!(
            "SELECT task_code, task_name, problem_statement, situation_codes, workflow_stage, first_action, stop_condition \
             FROM writing_task_queries ORDER BY id LIMIT {}",
            limit.max(1)
        ),
        |row| {
            Ok(ExternalWritingTaskQuery {
                task_code: row.get(0)?,
                task_name: row.get(1)?,
                problem_statement: row.get(2)?,
                situation_codes: row.get(3)?,
                workflow_stage: row.get(4)?,
                first_action: row.get(5)?,
                stop_condition: row.get(6)?,
            })
        },
    )
}

pub fn load_eval_signals(limit: usize) -> Result<Vec<ExternalEvalSignal>, String> {
    let path = configured_db_path()
        .ok_or_else(|| "FORGE_EXTERNAL_WRITING_DB is not configured.".to_string())?;
    let conn = open_read_only(&path)?;
    let mut stmt = conn
        .prepare(
            "SELECT code, failure_kind, description, is_default
             FROM eval_signals
             ORDER BY is_default DESC, code
             LIMIT ?1",
        )
        .map_err(|e| format!("Failed to prepare eval signal query: {}", e))?;
    let rows = stmt
        .query_map(rusqlite::params![limit.max(1) as i64], |row| {
            Ok(ExternalEvalSignal {
                code: row.get(0)?,
                failure_kind: row.get(1)?,
                description: row.get(2)?,
                is_default: row.get::<_, i64>(3)? != 0,
            })
        })
        .map_err(|e| format!("Failed to query eval signals: {}", e))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| format!("Failed to collect eval signals: {}", e))
}

pub fn load_prompt_templates(limit: usize) -> Result<Vec<ExternalPromptTemplate>, String> {
    let path = configured_db_path()
        .ok_or_else(|| "FORGE_EXTERNAL_WRITING_DB is not configured.".to_string())?;
    let conn = open_read_only(&path)?;
    let mut stmt = conn
        .prepare(
            "SELECT code, title, workflow_stage, summary
             FROM prompt_templates
             ORDER BY id
             LIMIT ?1",
        )
        .map_err(|e| format!("Failed to prepare prompt template query: {}", e))?;
    let rows = stmt
        .query_map(rusqlite::params![limit.max(1) as i64], |row| {
            Ok(ExternalPromptTemplate {
                code: row.get(0)?,
                title: row.get(1)?,
                workflow_stage: row.get(2)?,
                summary: row.get(3)?,
            })
        })
        .map_err(|e| format!("Failed to query prompt templates: {}", e))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| format!("Failed to collect prompt templates: {}", e))
}

pub fn search_context_bundle(
    query: &str,
    limit: usize,
) -> Result<ExternalWritingContextBundle, String> {
    let path = configured_db_path()
        .ok_or_else(|| "FORGE_EXTERNAL_WRITING_DB is not configured.".to_string())?;
    let conn = open_read_only(&path)?;
    let escaped = query.replace('\'', "''");
    let like = format!("%{}%", escaped);
    let task_queries = {
        let mut stmt = conn.prepare(
            "SELECT task_code, task_name, problem_statement, situation_codes, workflow_stage, first_action, stop_condition
             FROM writing_task_queries
             WHERE task_name LIKE ?1 OR problem_statement LIKE ?1 OR situation_codes LIKE ?1
             ORDER BY id
             LIMIT ?2",
        ).map_err(|e| format!("Failed to prepare task query search: {}", e))?;
        let rows = stmt
            .query_map(rusqlite::params![like, limit as i64], |row| {
                Ok(ExternalWritingTaskQuery {
                    task_code: row.get(0)?,
                    task_name: row.get(1)?,
                    problem_statement: row.get(2)?,
                    situation_codes: row.get(3)?,
                    workflow_stage: row.get(4)?,
                    first_action: row.get(5)?,
                    stop_condition: row.get(6)?,
                })
            })
            .map_err(|e| format!("Failed to search task queries: {}", e))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| format!("Failed to collect task queries: {}", e))?
    };

    let situation_rules = {
        let mut stmt = conn.prepare(
            "SELECT situation_code, rule_type, rule_code, rule_name, rule_summary, usage_note, local_path
             FROM situation_rule_reading_view
             WHERE situation_name LIKE ?1 OR situation_purpose LIKE ?1 OR rule_name LIKE ?1 OR rule_summary LIKE ?1 OR usage_note LIKE ?1
             ORDER BY relevance DESC
             LIMIT ?2",
        ).map_err(|e| format!("Failed to prepare situation rule search: {}", e))?;
        let rows = stmt
            .query_map(rusqlite::params![like, limit as i64], |row| {
                Ok(ExternalWritingRuleAsset {
                    situation_code: row.get(0)?,
                    workflow_stage: None,
                    rule_type: row.get(1)?,
                    rule_code: row.get(2)?,
                    rule_name: row.get(3)?,
                    rule_summary: row.get(4)?,
                    usage_note: row.get(5)?,
                    local_path: row.get(6)?,
                })
            })
            .map_err(|e| format!("Failed to search situation rules: {}", e))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| format!("Failed to collect situation rules: {}", e))?
    };

    let workflow_rules = {
        let mut stmt = conn.prepare(
            "SELECT workflow_stage, rule_type, rule_code, rule_name, rule_summary, usage_note, local_path
             FROM workflow_rule_reading_view
             WHERE workflow_stage LIKE ?1 OR rule_name LIKE ?1 OR rule_summary LIKE ?1 OR usage_note LIKE ?1
             ORDER BY workflow_stage
             LIMIT ?2",
        ).map_err(|e| format!("Failed to prepare workflow rule search: {}", e))?;
        let rows = stmt
            .query_map(rusqlite::params![like, limit as i64], |row| {
                Ok(ExternalWritingRuleAsset {
                    situation_code: None,
                    workflow_stage: row.get(0)?,
                    rule_type: row.get(1)?,
                    rule_code: row.get(2)?,
                    rule_name: row.get(3)?,
                    rule_summary: row.get(4)?,
                    usage_note: row.get(5)?,
                    local_path: row.get(6)?,
                })
            })
            .map_err(|e| format!("Failed to search workflow rules: {}", e))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| format!("Failed to collect workflow rules: {}", e))?
    };

    let references = {
        let sql = format!(
            "SELECT reference_id, situation, novel, chapter_no, chapter, why_selected, reading_task, summary, file_path,
                    scene_goal, opposition, leverage, reversal, cost, aftereffect
             FROM gold_references
             WHERE situation LIKE '{like}' OR novel LIKE '{like}' OR chapter LIKE '{like}' OR summary LIKE '{like}' OR why_selected LIKE '{like}'
             ORDER BY score DESC
             LIMIT {}",
            limit.max(1)
        );
        query_map_vec(&conn, &sql, |row| {
            Ok(ExternalWritingReference {
                reference_id: row.get(0)?,
                situation: row.get(1)?,
                novel: row.get(2)?,
                chapter_no: row.get(3)?,
                chapter: row.get(4)?,
                why_selected: row.get(5)?,
                reading_task: row.get(6)?,
                summary: row.get(7)?,
                file_path: row.get(8)?,
                scene_goal: row.get(9).ok(),
                opposition: row.get(10).ok(),
                leverage: row.get(11).ok(),
                reversal: row.get(12).ok(),
                cost: row.get(13).ok(),
                aftereffect: row.get(14).ok(),
            })
        })?
    };

    let reference_rules = {
        let sql = format!(
            "SELECT reference_id, situation, novel, chapter_no, chapter, rule_type, rule_code, rule_name, rule_summary, relevance, usage_note
             FROM reference_rule_reading_view
             WHERE situation LIKE '{like}' OR novel LIKE '{like}' OR chapter LIKE '{like}' OR rule_name LIKE '{like}' OR usage_note LIKE '{like}'
             ORDER BY relevance DESC
             LIMIT {}",
            (limit.max(1) * 3)
        );
        query_map_vec(&conn, &sql, |row| {
            Ok(ExternalReferenceRule {
                reference_id: row.get(0)?,
                situation: row.get(1)?,
                novel: row.get(2)?,
                chapter_no: row.get(3)?,
                chapter: row.get(4)?,
                rule_type: row.get(5)?,
                rule_code: row.get(6)?,
                rule_name: row.get(7)?,
                rule_summary: row.get(8)?,
                relevance: row.get(9)?,
                usage_note: row.get(10)?,
            })
        })?
    };

    Ok(ExternalWritingContextBundle {
        query: query.to_string(),
        task_queries,
        situation_rules,
        workflow_rules,
        references,
        reference_rules,
    })
}

pub fn render_context_bundle(bundle: &ExternalWritingContextBundle, char_budget: usize) -> String {
    let mut sections = Vec::new();

    if !bundle.task_queries.is_empty() {
        let lines = bundle
            .task_queries
            .iter()
            .map(|q| {
                format!(
                    "- [{}] {} | 阶段={} | 问题={} | 首动作={}",
                    q.task_code, q.task_name, q.workflow_stage, q.problem_statement, q.first_action
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## 外部写作任务入口\n{}", lines));
    }

    if !bundle.situation_rules.is_empty() {
        let lines = bundle
            .situation_rules
            .iter()
            .take(8)
            .map(|r| {
                format!(
                    "- [{}:{}] {} | 用法={}",
                    r.rule_type, r.rule_code, r.rule_name, r.usage_note
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## 外部局势规则映射\n{}", lines));
    }

    if !bundle.workflow_rules.is_empty() {
        let lines = bundle
            .workflow_rules
            .iter()
            .take(6)
            .map(|r| {
                format!(
                    "- [阶段:{}] [{}:{}] {} | 用法={}",
                    r.workflow_stage.clone().unwrap_or_default(),
                    r.rule_type,
                    r.rule_code,
                    r.rule_name,
                    r.usage_note
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## 外部工作流规则入口\n{}", lines));
    }

    if !bundle.references.is_empty() {
        let lines = bundle
            .references
            .iter()
            .take(5)
            .map(|r| {
                format!(
                    "- [{}] {} 第{}章《{}》 | {}",
                    r.situation, r.novel, r.chapter_no, r.chapter, r.why_selected
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## 外部参考样本\n{}", lines));
    }

    if !bundle.reference_rules.is_empty() {
        let lines = bundle
            .reference_rules
            .iter()
            .take(8)
            .map(|r| {
                format!(
                    "- [{} 第{}章] {}:{} | {}",
                    r.novel, r.chapter_no, r.rule_type, r.rule_code, r.usage_note
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("## 外部金样本规则映射\n{}", lines));
    }

    let text = sections.join("\n\n");
    if text.chars().count() <= char_budget {
        text
    } else {
        text.chars().take(char_budget).collect()
    }
}
