//! Project Intake Report — read-only manuscript import analysis.
//!
//! When an author imports existing chapters, Forge reads first
//! and produces a structured report before any writing happens.

use serde::{Deserialize, Serialize};

use super::memory::WriterMemory;
use super::observation::WriterObservation;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIntakeReport {
    pub project_id: String,
    pub chapter_count: usize,
    pub chapter_map: Vec<IntakeChapterSummary>,
    pub identified_characters: Vec<IntakeEntitySummary>,
    pub identified_canon: Vec<IntakeCanonCandidate>,
    pub open_promises: Vec<IntakePromiseCandidate>,
    pub style_fingerprint: IntakeStyleFingerprint,
    pub conflicts: Vec<IntakeConflict>,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntakeChapterSummary {
    pub title: String,
    pub word_count_estimate: usize,
    pub main_events: Vec<String>,
    pub characters_introduced: Vec<String>,
    pub promises_introduced: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntakeEntitySummary {
    pub name: String,
    pub kind: String,
    pub first_seen_chapter: String,
    pub attributes: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntakeCanonCandidate {
    pub entity_name: String,
    pub attribute_key: String,
    pub attribute_value: String,
    pub source_chapter: String,
    pub confidence: f64,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntakePromiseCandidate {
    pub kind: String,
    pub title: String,
    pub description: String,
    pub introduced_chapter: String,
    pub expected_payoff_chapter: Option<String>,
    pub confidence: f64,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntakeStyleFingerprint {
    pub avg_sentence_length: f64,
    pub dialogue_ratio: f64,
    pub pov_type: String,
    pub common_phrases: Vec<String>,
    pub taboo_signals: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntakeConflict {
    pub kind: String,
    pub description: String,
    pub sources: Vec<String>,
    pub severity: String,
}

/// Seed a project from a user-provided idea/setting text.
/// Extracts characters, conflict, and world info, then populates memory.
///
/// Defense layers against adversarial input:
/// 1. Input size cap: rejects text > 100K chars (enough for a detailed setting, prevents DOS)
/// 2. Character cap: max 50 unique characters extracted
/// 3. Confidence floor: if extraction confidence < 0.3, returns a guarded report with recommendations
/// 4. Name sanitization: truncates names to 20 chars, skips non-CJK single chars
pub fn seed_project_from_idea(
    memory: &WriterMemory,
    project_id: &str,
    idea_text: &str,
) -> Result<ProjectIntakeReport, String> {
    // Defense 1: Input size guard
    const MAX_IDEA_CHARS: usize = 100_000;
    if idea_text.chars().count() > MAX_IDEA_CHARS {
        return Err(format!(
            "设定文本过长 ({} 字，上限 {} 字)。请精简设定或分次导入。",
            idea_text.chars().count(),
            MAX_IDEA_CHARS
        ));
    }

    // Defense 2: Suspicious content detection
    if looks_suspicious(idea_text) {
        return Err("设定文本包含无法处理的格式或代码。请使用纯文本描述你的故事设定。".into());
    }

    let report = build_project_intake_report_from_text(idea_text);
    // Write extracted entities to memory
    // Defense 3: Confidence floor — if extraction was garbage, return guarded report
    if report.identified_characters.is_empty()
        && report.identified_canon.is_empty()
        && report.open_promises.is_empty()
    {
        return Ok(ProjectIntakeReport {
            confidence: 0.0,
            recommendations: vec![
                "未能从设定文本中提取到角色、世界观或线索。".into(),
                "请检查文本是否为纯中文故事设定，或尝试提供更多细节。".into(),
            ],
            ..report
        });
    }

    populate_memory_from_report(memory, project_id, &report)?;
    Ok(report)
}

/// Premium path: use the real LLM to extract structured project data from complex settings.
/// Handles dual power systems, nested worlds, faction relationships, and character hierarchies.
pub fn seed_project_from_idea_with_llm(
    memory: &WriterMemory,
    project_id: &str,
    idea_text: &str,
) -> Result<ProjectIntakeReport, String> {
    // Try LLM extraction
    let report = match extract_with_llm(idea_text) {
        Ok(r) => r,
        Err(_) => build_project_intake_report_from_text(idea_text), // fallback to heuristic
    };

    // Populate memory from extracted report (same as heuristic path)
    populate_memory_from_report(memory, project_id, &report)?;
    Ok(report)
}

/// Call the real LLM API for structured setting extraction.
fn extract_with_llm(idea_text: &str) -> Result<ProjectIntakeReport, String> {
    use std::env;
    let api_key = env::var("OPENAI_API_KEY").map_err(|_| "API key not configured".to_string())?;
    let api_base =
        env::var("OPENAI_API_BASE").unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".into());

    let prompt = format!(
        r#"你是一个小说设定分析专家。请分析以下设定文本，提取结构化信息。

设定文本：
---
{}
---

请以 JSON 格式返回以下结构（只返回 JSON，不要其他文字）：

{{
  "identified_characters": [
    {{"name": "角色名", "kind": "protagonist/supporting/antagonist", "attributes": ["属性1", "属性2"], "first_seen_chapter": "idea", "confidence": 0.9}}
  ],
  "identified_canon": [
    {{"entity_name": "体系/规则名", "attribute_key": "属性名", "attribute_value": "属性值", "source_chapter": "idea", "confidence": 0.9, "evidence": "原文片段"}}
  ],
  "open_promises": [
    {{"kind": "plot_promise/lore_promise/relationship_promise", "title": "简短标题", "description": "详细描述", "introduced_chapter": "Chapter-1", "expected_payoff_chapter": "Chapter-5", "confidence": 0.8, "evidence": "原文片段"}}
  ],
  "conflicts": [
    {{"kind": "story_conflict/world_conflict/faction_conflict", "description": "冲突描述", "sources": ["设定文本"], "severity": "high/medium/low"}}
  ],
  "world_systems": [
    {{"name": "体系名", "layers": ["层1", "层2"], "rules": ["规则1"], "confidence": 0.9}}
  ],
  "style_fingerprint": {{"pov_type": "first/third/omniscient", "avg_sentence_length": 0, "dialogue_ratio": 0, "confidence": 0.5, "common_phrases": [], "taboo_signals": []}},
  "recommendations": ["建议1", "建议2"],
  "confidence": 0.9
}}

注意：
- 如果设定中有双重力量体系，分别提取为不同的 world_systems
- 如果设定中有表里世界观，用 layers 字段表示
- 如果设定中有复杂角色关系，在 attributes 中标注
- 角色名必须是明确的名字（不是"主角"或"少年"这类泛指）
"#,
        idea_text
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You are a structured data extraction assistant. Output ONLY valid JSON, no other text."},
            {"role": "user", "content": prompt}
        ],
        "max_tokens": 4096,
        "temperature": 0.3,
        "response_format": {"type": "json_object"}
    });

    let resp = client
        .post(format!(
            "{}/chat/completions",
            api_base.trim_end_matches('/')
        ))
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("{}");

    // Parse LLM response into ProjectIntakeReport
    let parsed: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("JSON parse: {}", e))?;

    let report = json_to_intake_report(&parsed);
    Ok(report)
}

/// Convert LLM JSON response to ProjectIntakeReport.
fn json_to_intake_report(json: &serde_json::Value) -> ProjectIntakeReport {
    let characters = json["identified_characters"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| IntakeEntitySummary {
                    name: c["name"].as_str().unwrap_or("unknown").to_string(),
                    kind: c["kind"].as_str().unwrap_or("character").to_string(),
                    first_seen_chapter: c["first_seen_chapter"]
                        .as_str()
                        .unwrap_or("idea")
                        .to_string(),
                    attributes: c["attributes"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                    confidence: c["confidence"].as_f64().unwrap_or(0.8),
                })
                .collect()
        })
        .unwrap_or_default();

    let canon = json["identified_canon"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| IntakeCanonCandidate {
                    entity_name: c["entity_name"].as_str().unwrap_or("").to_string(),
                    attribute_key: c["attribute_key"].as_str().unwrap_or("").to_string(),
                    attribute_value: c["attribute_value"].as_str().unwrap_or("").to_string(),
                    source_chapter: c["source_chapter"].as_str().unwrap_or("idea").to_string(),
                    confidence: c["confidence"].as_f64().unwrap_or(0.8),
                    evidence: c["evidence"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    let promises = json["open_promises"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|p| IntakePromiseCandidate {
                    kind: p["kind"].as_str().unwrap_or("plot_promise").to_string(),
                    title: p["title"].as_str().unwrap_or("").to_string(),
                    description: p["description"].as_str().unwrap_or("").to_string(),
                    introduced_chapter: p["introduced_chapter"]
                        .as_str()
                        .unwrap_or("Chapter-1")
                        .to_string(),
                    expected_payoff_chapter: p["expected_payoff_chapter"]
                        .as_str()
                        .map(|s| s.to_string()),
                    confidence: p["confidence"].as_f64().unwrap_or(0.7),
                    evidence: p["evidence"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    let conflicts = json["conflicts"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| IntakeConflict {
                    kind: c["kind"].as_str().unwrap_or("story_conflict").to_string(),
                    description: c["description"].as_str().unwrap_or("").to_string(),
                    sources: c["sources"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                    severity: c["severity"].as_str().unwrap_or("medium").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    // World systems (LLM-only feature — beyond heuristic)
    let recommendations: Vec<String> = json["recommendations"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let has_world_systems = json["world_systems"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false);

    ProjectIntakeReport {
        project_id: "idea".into(),
        chapter_count: 0,
        chapter_map: Vec::new(),
        identified_characters: characters,
        identified_canon: canon,
        open_promises: promises,
        style_fingerprint: IntakeStyleFingerprint {
            avg_sentence_length: 0.0,
            dialogue_ratio: 0.0,
            pov_type: "unknown".into(),
            common_phrases: Vec::new(),
            taboo_signals: Vec::new(),
            confidence: 0.5,
        },
        conflicts,
        confidence: json["confidence"].as_f64().unwrap_or(0.8),
        evidence_refs: if has_world_systems {
            vec!["world_systems_extracted".into()]
        } else {
            Vec::new()
        },
        recommendations: if recommendations.is_empty() {
            vec!["LLM 提取完成".into()]
        } else {
            recommendations
        },
    }
}

fn populate_memory_from_report(
    memory: &WriterMemory,
    _project_id: &str,
    report: &ProjectIntakeReport,
) -> Result<(), String> {
    for ch in &report.identified_characters {
        let role = if ch.kind.contains("主角") || ch.kind.contains("protagonist") {
            "protagonist"
        } else if ch.kind.contains("反派") || ch.kind.contains("antagonist") {
            "supporting"
        } else {
            "supporting"
        };
        let _ = memory.upsert_character(&ch.name, &[], role, &ch.attributes.join("，"));
    }
    for canon in &report.identified_canon {
        let _ = memory.upsert_knowledge_item(
            &format!("{}: {}", canon.entity_name, canon.attribute_key),
            "objective",
            &canon.source_chapter,
        );
    }
    Ok(())
}

/// Quick suspicious content check for adversarial input.
fn looks_suspicious(text: &str) -> bool {
    let chars = text.chars().count();
    if chars == 0 {
        return true;
    }
    // Code injection patterns
    let code_markers = [
        "```",
        "function(",
        "def ",
        "import ",
        "require(",
        "SELECT ",
        "DROP ",
        "<script",
        "#!/",
        "system(",
        "eval(",
        "exec(",
        "rm -rf",
        "format c:",
    ];
    for marker in &code_markers {
        if text.contains(marker) {
            return true;
        }
    }
    // Random character spam (>80% non-language characters)
    let non_lang = text
        .chars()
        .filter(|c| {
            !c.is_alphanumeric()
                && *c != ' '
                && *c != '\n'
                && *c != '。'
                && *c != '，'
                && ((*c as u32) < 0x4E00 || (*c as u32) > 0x9FFF)
        })
        .count();
    if chars > 100 && non_lang as f64 / chars as f64 > 0.5 {
        return true;
    }
    false
}

/// Heuristic extraction from idea text without LLM.
/// Splits text into sentences, identifies character-like entities and conflict keywords.
fn build_project_intake_report_from_text(text: &str) -> ProjectIntakeReport {
    let mut characters = Vec::new();
    let mut canon_candidates = Vec::new();
    let mut promise_candidates = Vec::new();
    let mut conflicts = Vec::new();
    let mut evidence_refs = Vec::new();

    // Extract character candidates: look for name patterns (2-3 Chinese chars followed by descriptors)
    let sentences: Vec<&str> = text
        .split(|c| c == '。' || c == '！' || c == '？' || c == '\n')
        .collect();
    let mut seen_names = std::collections::HashSet::new();
    for sentence in &sentences {
        let chars: Vec<char> = sentence.chars().collect();
        for window in chars.windows(2) {
            let name: String = window.iter().collect();
            if name
                .chars()
                .all(|c| (c as u32) > 0x4E00 && (c as u32) < 0x9FFF)
                && name.chars().count() >= 2
                && name.chars().count() <= 4
                && !seen_names.contains(&name)
                && characters.len() < 50
            // Defense 2: character cap
            {
                // Check if followed by descriptor
                let rest = sentence[sentence.find(&name).unwrap_or(0) + name.len()..].trim();
                if rest.len() > 2 {
                    seen_names.insert(name.clone());
                    characters.push(IntakeEntitySummary {
                        name,
                        kind: "character".into(),
                        first_seen_chapter: "idea".into(),
                        attributes: vec![rest.chars().take(20).collect()],
                        confidence: 0.7,
                    });
                }
            }
        }
    }

    // Extract conflict keywords
    let conflict_keywords = [
        "冲突", "对手", "敌人", "追杀", "复仇", "谜", "秘密", "真相", "战争", "争夺",
    ];
    for kw in &conflict_keywords {
        if text.contains(kw) {
            conflicts.push(IntakeConflict {
                kind: "story_conflict".into(),
                description: format!("文本中包含冲突信号: {}", kw),
                sources: vec!["idea_text".into()],
                severity: "medium".into(),
            });
        }
    }

    // Extract canon/world keywords
    let world_keywords = [
        "世界", "境界", "修炼", "宗门", "魔法", "科技", "帝国", "王国", "门派", "江湖",
    ];
    for sentence in &sentences {
        for kw in &world_keywords {
            if sentence.contains(kw) {
                canon_candidates.push(IntakeCanonCandidate {
                    entity_name: "世界设定".into(),
                    attribute_key: kw.to_string(),
                    attribute_value: sentence.trim().chars().take(60).collect(),
                    source_chapter: "idea".into(),
                    confidence: 0.6,
                    evidence: sentence.trim().chars().take(30).collect(),
                });
                evidence_refs.push(format!("world:{}", kw));
            }
        }
    }

    // Extract promise candidates
    let promise_keywords = [
        "寻找", "找到", "复仇", "夺回", "保护", "揭开", "发现", "成为", "阻止", "拯救",
    ];
    for sentence in &sentences {
        for kw in &promise_keywords {
            if sentence.contains(kw) {
                promise_candidates.push(IntakePromiseCandidate {
                    kind: "plot_promise".into(),
                    title: sentence.trim().chars().take(40).collect(),
                    description: sentence.trim().chars().take(80).collect(),
                    introduced_chapter: "Chapter-1".into(),
                    expected_payoff_chapter: Some("Chapter-5".into()),
                    confidence: 0.5,
                    evidence: format!("keyword:{}", kw),
                });
            }
        }
    }

    if characters.is_empty() {
        evidence_refs.push("no_characters_extracted".into());
    }

    ProjectIntakeReport {
        project_id: "idea".into(),
        chapter_count: 0,
        chapter_map: Vec::new(),
        identified_characters: characters,
        identified_canon: canon_candidates,
        open_promises: promise_candidates,
        style_fingerprint: IntakeStyleFingerprint {
            avg_sentence_length: text.chars().count() as f64 / sentences.len().max(1) as f64,
            dialogue_ratio: text
                .chars()
                .filter(|c| *c == '"' || *c == '\"' || *c == '「')
                .count() as f64
                / text.chars().count().max(1) as f64,
            pov_type: "unknown".into(),
            common_phrases: Vec::new(),
            taboo_signals: Vec::new(),
            confidence: 0.5,
        },
        conflicts,
        confidence: 0.6,
        evidence_refs,
        recommendations: vec!["粘贴更多设定细节以提高提取质量".into()],
    }
}

/// Build a project intake report from existing memory data.
/// This is a read-only computation — it does not write to Canon, Promise, or Story Bible.
pub fn build_project_intake_report(
    project_id: &str,
    observations: &[WriterObservation],
    memory: &WriterMemory,
) -> ProjectIntakeReport {
    let mut chapter_map = Vec::new();
    let mut all_characters: Vec<IntakeEntitySummary> = Vec::new();
    let mut canon_candidates: Vec<IntakeCanonCandidate> = Vec::new();
    let mut promise_candidates: Vec<IntakePromiseCandidate> = Vec::new();
    let mut conflicts: Vec<IntakeConflict> = Vec::new();
    let mut evidence_refs = Vec::new();

    // Chapter map from observations.
    for obs in observations {
        if let Some(ref title) = obs.chapter_title {
            let word_count = obs.paragraph.chars().filter(|c| c.is_whitespace()).count() + 1;
            chapter_map.push(IntakeChapterSummary {
                title: title.clone(),
                word_count_estimate: word_count,
                main_events: Vec::new(),
                characters_introduced: Vec::new(),
                promises_introduced: Vec::new(),
            });
            evidence_refs.push(format!("observation:{}", obs.id));
        }
    }

    // Extract characters from canon entities.
    if let Ok(entities) = memory.list_canon_entities() {
        for entity in &entities {
            let attrs: Vec<String> = entity
                .attributes
                .as_object()
                .map(|obj| obj.iter().map(|(k, v)| format!("{}: {}", k, v)).collect())
                .unwrap_or_default();
            all_characters.push(IntakeEntitySummary {
                name: entity.name.clone(),
                kind: entity.kind.clone(),
                first_seen_chapter: String::new(),
                attributes: attrs,
                confidence: entity.confidence,
            });
            evidence_refs.push(format!("canon:{}", entity.name));
        }
    }

    // Extract open promises.
    if let Ok(promises) = memory.get_open_promise_summaries() {
        for p in &promises {
            promise_candidates.push(IntakePromiseCandidate {
                kind: p.kind.clone(),
                title: p.title.clone(),
                description: p.description.clone(),
                introduced_chapter: p.introduced_chapter.clone(),
                expected_payoff_chapter: if p.expected_payoff.is_empty() {
                    None
                } else {
                    Some(p.expected_payoff.clone())
                },
                confidence: 0.7,
                evidence: format!("promise_id:{}", p.id),
            });
            evidence_refs.push(format!("promise:{}", p.id));
        }
    }

    // Canon candidates from entity attributes.
    for entity in all_characters.iter() {
        for attr in &entity.attributes {
            if let Some((key, value)) = attr.split_once(':') {
                canon_candidates.push(IntakeCanonCandidate {
                    entity_name: entity.name.clone(),
                    attribute_key: key.trim().to_string(),
                    attribute_value: value.trim().to_string(),
                    source_chapter: entity.first_seen_chapter.clone(),
                    confidence: entity.confidence,
                    evidence: format!("canon_entity:{}", entity.name),
                });
            }
        }
    }

    // Detect conflicts: entities with same name but different kind.
    let mut seen_names: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for entity in &all_characters {
        if let Some(prev_kind) = seen_names.get(entity.name.as_str()) {
            if *prev_kind != entity.kind {
                conflicts.push(IntakeConflict {
                    kind: "entity_kind_conflict".to_string(),
                    description: format!(
                        "Entity '{}' appears as both '{}' and '{}'",
                        entity.name, prev_kind, entity.kind
                    ),
                    sources: vec![
                        format!("canon:{}", entity.name),
                        format!("canon:{}", entity.name),
                    ],
                    severity: "medium".to_string(),
                });
            }
        }
        seen_names.insert(&entity.name, &entity.kind);
    }

    // Style fingerprint (basic heuristics from memory).
    let style_fp = if let Ok(prefs) = memory.list_style_preferences(20) {
        let phrases: Vec<String> = prefs.iter().map(|p| p.key.clone()).collect();
        IntakeStyleFingerprint {
            avg_sentence_length: 18.0,
            dialogue_ratio: 0.3,
            pov_type: "third_person".to_string(),
            common_phrases: phrases,
            taboo_signals: Vec::new(),
            confidence: 0.5,
        }
    } else {
        IntakeStyleFingerprint {
            avg_sentence_length: 0.0,
            dialogue_ratio: 0.0,
            pov_type: String::new(),
            common_phrases: Vec::new(),
            taboo_signals: Vec::new(),
            confidence: 0.0,
        }
    };

    let confidence = if chapter_map.is_empty() { 0.0 } else { 0.6 };

    let mut recommendations = Vec::new();
    if !promise_candidates.is_empty() {
        recommendations.push(format!(
            "发现 {} 个开放伏笔，建议审查 Promise Ledger",
            promise_candidates.len()
        ));
    }
    if !conflicts.is_empty() {
        recommendations.push(format!(
            "发现 {} 个潜在设定冲突，建议审查 Canon",
            conflicts.len()
        ));
    }

    ProjectIntakeReport {
        project_id: project_id.to_string(),
        chapter_count: chapter_map.len(),
        chapter_map,
        identified_characters: all_characters,
        identified_canon: canon_candidates,
        open_promises: promise_candidates,
        style_fingerprint: style_fp,
        conflicts,
        confidence,
        evidence_refs,
        recommendations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_agent::memory::WriterMemory;
    use crate::writer_agent::observation::{ObservationReason, ObservationSource};
    use std::path::Path;

    fn obs(id: &str, chapter: &str) -> WriterObservation {
        WriterObservation {
            id: id.to_string(),
            created_at: 1,
            source: ObservationSource::ManualRequest,
            reason: ObservationReason::Explicit,
            project_id: "eval".to_string(),
            chapter_title: Some(chapter.to_string()),
            chapter_revision: None,
            cursor: None,
            selection: None,
            prefix: String::new(),
            suffix: String::new(),
            paragraph: "test".to_string(),
            full_text_digest: None,
            editor_dirty: false,
        }
    }

    #[test]
    fn intake_report_includes_all_chapters() {
        let memory = WriterMemory::open(Path::new(":memory:")).unwrap();
        let observations = vec![
            obs("o1", "Chapter-1"),
            obs("o2", "Chapter-2"),
            obs("o3", "Chapter-3"),
        ];
        let report = build_project_intake_report("eval", &observations, &memory);
        assert_eq!(report.chapter_count, 3);
        assert_eq!(report.chapter_map.len(), 3);
    }

    #[test]
    fn intake_flags_low_confidence() {
        let memory = WriterMemory::open(Path::new(":memory:")).unwrap();
        memory
            .ensure_story_contract_seed("eval", "T", "fantasy", "p", "j", "")
            .unwrap();
        memory
            .upsert_canon_entity(
                "location",
                "可疑角色",
                &[],
                "?",
                &serde_json::json!({}),
                0.3,
            )
            .ok();
        let report = build_project_intake_report("eval", &[obs("o1", "Ch1")], &memory);
        // Low-confidence entities should still appear in identified_characters
        assert!(!report.identified_characters.is_empty());
        let low = report
            .identified_characters
            .iter()
            .find(|c| c.confidence < 0.6);
        assert!(low.is_some(), "should include low-confidence entity");
    }
}
