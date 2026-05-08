//! Prompt caching — reduces token cost for repeated system prompts.
//! Ported from Claw Code `api/src/prompt_cache.rs`.
//!
//! Uses FNV-1a hashing to fingerprint (system_prompt + tools) pairs.
//! Tracks hits/misses so the agent loop can decide whether to add
//! cache breakpoints (Anthropic) or rely on server-side caching (OpenRouter).

use std::collections::HashMap;
use std::time::{Duration, Instant};

const DEFAULT_TTL: Duration = Duration::from_secs(5 * 60); // 5 min

#[derive(Debug, Clone)]
pub struct PromptCacheConfig {
    pub ttl: Duration,
    pub enabled: bool,
}

impl Default for PromptCacheConfig {
    fn default() -> Self {
        Self {
            ttl: DEFAULT_TTL,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PromptCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub tokens_saved: u64,
}

/// Tracks which prompts have been recently sent, avoiding redundant
/// system prompt tokens when the fingerprint hasn't changed.
pub struct PromptCache {
    config: PromptCacheConfig,
    entries: HashMap<u64, CacheEntry>,
    stats: PromptCacheStats,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    created: Instant,
}

impl PromptCache {
    pub fn new(config: PromptCacheConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
            stats: PromptCacheStats::default(),
        }
    }

    /// FNV-1a hash for content fingerprinting.
    pub fn fingerprint(system: &str, tools_json: &str) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for &b in system.as_bytes().iter().chain(tools_json.as_bytes()) {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x00000100000001b3);
        }
        hash
    }

    /// Check if a prompt is cached. Returns Some(tokens_saved) on hit.
    pub fn check(&mut self, system: &str, tools_json: &str) -> Option<u64> {
        if !self.config.enabled {
            return None;
        }
        let fp = Self::fingerprint(system, tools_json);
        let now = Instant::now();

        // Evict expired entries
        self.entries
            .retain(|_, e| now.duration_since(e.created) < self.config.ttl);

        if let Some(entry) = self.entries.get(&fp) {
            if now.duration_since(entry.created) < self.config.ttl {
                self.stats.hits += 1;
                let saved = system.len() as u64 / 3 + tools_json.len() as u64 / 3;
                self.stats.tokens_saved += saved;
                return Some(saved);
            }
        }

        self.stats.misses += 1;
        self.entries.insert(fp, CacheEntry { created: now });
        None
    }

    pub fn stats(&self) -> &PromptCacheStats {
        &self.stats
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_deterministic() {
        let a = PromptCache::fingerprint("hello", "{}");
        let b = PromptCache::fingerprint("hello", "{}");
        assert_eq!(a, b);
    }

    #[test]
    fn test_fingerprint_different() {
        let a = PromptCache::fingerprint("hello", "{}");
        let b = PromptCache::fingerprint("world", "{}");
        assert_ne!(a, b);
    }

    #[test]
    fn test_cache_hit() {
        let mut cache = PromptCache::new(PromptCacheConfig::default());
        assert!(cache.check("sys", "[]").is_none()); // miss
        assert!(cache.check("sys", "[]").is_some()); // hit
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn test_cache_disabled() {
        let mut cache = PromptCache::new(PromptCacheConfig {
            enabled: false,
            ..Default::default()
        });
        assert!(cache.check("sys", "[]").is_none());
        assert!(cache.check("sys", "[]").is_none()); // still miss
    }
}
