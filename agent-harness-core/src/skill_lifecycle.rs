use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::str::FromStr;

/// A skill extracted from agent experience.
/// Mirrors Hermes SKILL.md structure + CowAgent agent_skills table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: SkillCategory,
    pub content: String,
    pub triggers: Vec<String>,
    pub confidence: f64,
    pub usage_count: u32,
    pub last_used: Option<String>,
    pub created_at: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SkillCategory {
    Style,
    Character,
    Pacing,
    Preference,
    PlotStructure,
    Dialogue,
    Description,
    WorldBuilding,
    Custom(String),
}

impl std::str::FromStr for SkillCategory {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "style" => Ok(SkillCategory::Style),
            "character" => Ok(SkillCategory::Character),
            "pacing" => Ok(SkillCategory::Pacing),
            "preference" => Ok(SkillCategory::Preference),
            "plot_structure" => Ok(SkillCategory::PlotStructure),
            "dialogue" => Ok(SkillCategory::Dialogue),
            "description" => Ok(SkillCategory::Description),
            "world_building" => Ok(SkillCategory::WorldBuilding),
            other => Ok(SkillCategory::Custom(other.to_string())),
        }
    }
}

/// The Curator maintains the skill collection.
/// Ported from Hermes Agent `agent/curator.py`.
pub struct SkillCurator {
    pub skills: HashMap<String, Skill>,
    pub config: CuratorConfig,
}

#[derive(Debug, Clone)]
pub struct CuratorConfig {
    pub max_skills: usize,
    pub decay_days: i64,
    pub min_confidence: f64,
}

impl Default for CuratorConfig {
    fn default() -> Self {
        Self {
            max_skills: 200,
            decay_days: 90,
            min_confidence: 0.3,
        }
    }
}

impl SkillCurator {
    pub fn new(config: CuratorConfig) -> Self {
        Self {
            skills: HashMap::new(),
            config,
        }
    }

    pub fn upsert_skill(&mut self, skill: Skill) {
        self.skills.insert(skill.id.clone(), skill);
    }

    /// Find skills matching the given context by trigger + category keywords.
    pub fn find_relevant(&self, context: &str, max_results: usize) -> Vec<&Skill> {
        let mut matches: Vec<(&Skill, f64)> = self
            .skills
            .values()
            .filter(|s| s.active)
            .map(|s| {
                let score = self.match_score(s, context);
                (s, score)
            })
            .collect();
        matches.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        matches.truncate(max_results);
        matches.into_iter().map(|(s, _)| s).collect()
    }

    fn match_score(&self, skill: &Skill, context: &str) -> f64 {
        let mut score = 0.0;
        for trigger in &skill.triggers {
            if context.contains(trigger.as_str()) {
                score += 0.4;
            }
        }
        score += skill.confidence * 0.2;
        score.min(1.0)
    }

    /// Decay inactive skills, prune excess.
    /// Called periodically — mirrors Hermes Curator + CowAgent consolidation.
    pub fn curate(&mut self) -> CurationReport {
        let mut report = CurationReport::default();
        let now = Utc::now();
        let threshold = now - Duration::days(self.config.decay_days);

        // Phase 1: Decay
        for skill in self.skills.values_mut() {
            if let Some(ref last) = skill.last_used {
                if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(last, "%Y-%m-%dT%H:%M:%S") {
                    let dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc);
                    if dt < threshold && skill.confidence < 0.5 {
                        skill.active = false;
                        report.decayed += 1;
                    }
                }
            }
        }

        // Phase 2: Prune excess
        let mut active: Vec<&mut Skill> = self.skills.values_mut().filter(|s| s.active).collect();
        if active.len() > self.config.max_skills {
            active.sort_by(|a, b| b.last_used.cmp(&a.last_used));
            for s in active.iter_mut().skip(self.config.max_skills) {
                s.active = false;
                report.pruned += 1;
            }
        }

        report
    }

    pub fn record_usage(&mut self, skill_id: &str) {
        if let Some(s) = self.skills.get_mut(skill_id) {
            s.usage_count += 1;
            s.last_used = Some(Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string());
            s.confidence = (s.confidence + 0.05).min(1.0);
        }
    }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct CurationReport {
    pub decayed: u32,
    pub pruned: u32,
}

/// Convert HermesDB agent_skills row to Skill struct.
pub fn skill_from_db(id: i64, text: &str, category: &str, active: bool, created_at: &str) -> Skill {
    Skill {
        id: format!("skill_{}", id),
        name: text.chars().take(60).collect(),
        description: text.to_string(),
        category: SkillCategory::from_str(category).unwrap_or_else(|never| match never {}),
        content: text.to_string(),
        triggers: Vec::new(),
        confidence: 0.5,
        usage_count: 0,
        last_used: Some(created_at.to_string()),
        created_at: created_at.to_string(),
        active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(id: &str, triggers: Vec<&str>, confidence: f64) -> Skill {
        Skill {
            id: id.into(),
            name: id.into(),
            description: "test".into(),
            category: SkillCategory::Style,
            content: "test".into(),
            triggers: triggers.iter().map(|s| s.to_string()).collect(),
            confidence,
            usage_count: 0,
            last_used: Some(Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string()),
            created_at: Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            active: true,
        }
    }

    #[test]
    fn test_find_relevant_by_trigger() {
        let mut c = SkillCurator::new(CuratorConfig::default());
        c.upsert_skill(make_skill("s1", vec!["打斗"], 0.8));
        c.upsert_skill(make_skill("s2", vec!["对话"], 0.5));
        let m = c.find_relevant("主角和反派打斗了起来", 3);
        assert!(!m.is_empty());
        assert_eq!(m[0].id, "s1");
    }

    #[test]
    fn test_record_usage() {
        let mut c = SkillCurator::new(CuratorConfig::default());
        c.upsert_skill(make_skill("s1", vec![], 0.5));
        c.record_usage("s1");
        assert!(c.skills["s1"].confidence > 0.5);
        assert_eq!(c.skills["s1"].usage_count, 1);
    }

    #[test]
    fn test_curate_decays_old_skills() {
        let mut c = SkillCurator::new(CuratorConfig {
            decay_days: 1,
            ..Default::default()
        });
        let old = Skill {
            id: "old".into(),
            name: "old".into(),
            description: "t".into(),
            category: SkillCategory::Style,
            content: "t".into(),
            triggers: vec![],
            confidence: 0.2,
            usage_count: 0,
            last_used: Some("2020-01-01T00:00:00".into()),
            created_at: "2020-01-01T00:00:00".into(),
            active: true,
        };
        c.upsert_skill(old);
        let r = c.curate();
        assert_eq!(r.decayed, 1);
    }
}
