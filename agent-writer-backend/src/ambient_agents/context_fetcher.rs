use agent_harness_core::ambient::{AgentOutput, AmbientAgent, EditorEvent};
use async_trait::async_trait;
use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Shared context cache readable by all ambient agents.
/// Populated silently by ContextFetcherAgent on keyword/character detection.
#[derive(Debug, Clone, Default)]
pub struct ContextCache {
    pub lore_entries: HashMap<String, Vec<String>>,
    pub outline_map: HashMap<String, String>,
    pub last_updated: u64,
}

pub struct ContextFetcherAgent {
    pub app: tauri::AppHandle,
    pub cache: Arc<Mutex<ContextCache>>,
}

#[async_trait]
impl AmbientAgent for ContextFetcherAgent {
    fn name(&self) -> &str {
        "context-fetcher"
    }

    fn subscribed_events(&self) -> Vec<String> {
        vec!["keyword_detected".into(), "chapter_switched".into()]
    }

    async fn process(&self, event: EditorEvent, _cancel: CancellationToken) -> Option<AgentOutput> {
        match event {
            EditorEvent::KeywordDetected {
                keywords, chapter, ..
            } => {
                let mut cache = self.cache.lock().await;
                let mut first_card = None;
                for kw in &keywords {
                    if cache.lore_entries.contains_key(kw) {
                        if first_card.is_none() {
                            if let Some(content) = cache
                                .lore_entries
                                .get(kw)
                                .and_then(|entries| entries.first())
                            {
                                first_card = Some((kw.clone(), content.clone()));
                            }
                        }
                        continue;
                    }
                    match crate::storage::load_lorebook(&self.app) {
                        Ok(entries) => {
                            let matches: Vec<String> = entries
                                .iter()
                                .filter(|e| e.keyword.contains(kw) || kw.contains(&e.keyword))
                                .map(|e| e.content.clone())
                                .collect();
                            if first_card.is_none() {
                                if let Some(content) = matches.first() {
                                    first_card = Some((kw.clone(), content.clone()));
                                }
                            }
                            cache.lore_entries.insert(kw.clone(), matches);
                        }
                        Err(e) => tracing::warn!("Context fetcher skipped lorebook: {}", e),
                    }
                }
                if let Entry::Vacant(entry) = cache.outline_map.entry(chapter.clone()) {
                    match crate::storage::load_outline(&self.app) {
                        Ok(nodes) => {
                            if let Some(node) =
                                nodes.iter().find(|n| n.chapter_title == *entry.key())
                            {
                                entry.insert(node.summary.clone());
                            }
                        }
                        Err(e) => tracing::warn!("Context fetcher skipped outline: {}", e),
                    }
                }
                cache.last_updated = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                if let Some((keyword, content)) = first_card {
                    return Some(AgentOutput::EntityCard {
                        keyword,
                        content,
                        chapter,
                    });
                }
            }
            EditorEvent::ChapterSwitched { to, .. } => {
                let mut cache = self.cache.lock().await;
                if let Entry::Vacant(entry) = cache.outline_map.entry(to) {
                    match crate::storage::load_outline(&self.app) {
                        Ok(nodes) => {
                            if let Some(node) =
                                nodes.iter().find(|n| n.chapter_title == *entry.key())
                            {
                                entry.insert(node.summary.clone());
                            }
                        }
                        Err(e) => tracing::warn!("Context fetcher skipped outline: {}", e),
                    }
                }
            }
            _ => {}
        }
        None // Pure background caching — no UI output
    }
}
