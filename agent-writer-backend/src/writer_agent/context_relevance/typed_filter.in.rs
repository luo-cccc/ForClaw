use super::context::ContextSource;
use super::memory::WriterMemory;

/// Pre-filter evidence sources through story impact radius.
///
/// Returns only sources that relate to an impacted entity or plot node.
/// Falls back to all sources when impact radius computation is unavailable.
pub fn story_impact_filter(
    sources: &[ContextSource],
    memory: &WriterMemory,
    chapter_title: &str,
) -> Vec<ContextSource> {
    // Attempt to identify impacted entities for this chapter.
    let has_active_entities = memory
        .list_characters(None)
        .ok()
        .map(|chars| {
            chars
                .iter()
                .any(|c| memory.get_active_state(c.id, chapter_title).ok().flatten().is_some())
        })
        .unwrap_or(false);

    let has_chapter_promises = memory
        .list_scenes_by_chapter(chapter_title)
        .ok()
        .map(|scenes| {
            scenes.iter().any(|scene| {
                memory
                    .get_scene_obligations(scene.id)
                    .ok()
                    .flatten()
                    .map(|obl| !obl.promise_ids.is_empty())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let has_pending = memory
        .get_open_promises()
        .ok()
        .map(|p| !p.is_empty())
        .unwrap_or(false);

    // If we can identify impacted entities / promises, only keep
    // story-level sources; otherwise return everything unchanged.
    if has_active_entities || has_chapter_promises || has_pending {
        let filtered: Vec<ContextSource> = sources
            .iter()
            .filter(|s| matches!(s,
                ContextSource::ChapterMission
                | ContextSource::NextBeat
                | ContextSource::ResultFeedback
                | ContextSource::CanonSlice
                | ContextSource::PromiseSlice
                | ContextSource::DecisionSlice
                | ContextSource::BookState
                | ContextSource::ArcSnapshot
                | ContextSource::VolumeSnapshot
                | ContextSource::OutlineSlice
                | ContextSource::StoryImpactRadius
                | ContextSource::ReaderCompensation
                | ContextSource::ProjectBrief
                | ContextSource::PreviousChapter
            ))
            .cloned()
            .collect();
        return filtered;
    }

    sources.to_vec() // fallback: return all
}

pub struct TypedFilterResult {
    pub entity_boost: f64,
    pub knowledge_boost: f64,
    pub scene_boost: f64,
    pub reasons: Vec<String>,
}

impl TypedFilterResult {
    pub fn neutral() -> Self {
        Self { entity_boost: 1.0, knowledge_boost: 1.0, scene_boost: 1.0, reasons: Vec::new() }
    }
    pub fn total_multiplier(&self) -> f64 {
        self.entity_boost * self.knowledge_boost * self.scene_boost
    }
}

pub fn apply_typed_filter(source_text: &str, chapter_title: &str, memory: &WriterMemory) -> TypedFilterResult {
    let mut result = TypedFilterResult::neutral();
    if let Ok(characters) = memory.list_characters(None) {
        for c in &characters {
            if source_text.contains(&c.name) {
                if c.role_type == "protagonist" {
                    result.entity_boost *= 1.3;
                    result.reasons.push(format!("protagonist:{}", c.name));
                }
                if let Ok(Some(state)) = memory.get_active_state(c.id, chapter_title) {
                    if let Some(commitments) = state.core_commitments.as_array() {
                        if !commitments.is_empty() {
                            result.entity_boost *= 1.15;
                            result.reasons.push(format!("pending_commitments:{}", c.name));
                        }
                    }
                }
                break;
            }
        }
    }
    if let Ok(items) = memory.list_knowledge_items(Some("objective")) {
        for item in &items {
            if source_text.contains(&item.topic) {
                result.knowledge_boost *= 1.2;
                result.reasons.push(format!("knowledge_topic:{}", item.topic));
                break;
            }
        }
    }
    if let Ok(scenes) = memory.list_scenes_by_chapter(chapter_title) {
        for scene in &scenes {
            if let Ok(Some(obl)) = memory.get_scene_obligations(scene.id) {
                if !obl.promise_ids.is_empty() || !obl.payoff_targets.is_empty() {
                    result.scene_boost *= 1.1;
                    result.reasons.push(format!("scene_obligations:{}", scene.id));
                    break;
                }
            }
        }
    }
    result
}
