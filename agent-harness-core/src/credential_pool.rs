//! Credential pool with failover rotation.
//! Ported from Hermes Agent `credential_pool.py`.
//!
//! Supports multiple API keys per provider with:
//! - fill_first / round_robin / random strategies
//! - exhaustion cooldown (1 hour default)
//! - automatic failover to next credential on 401/429/5xx

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PoolStrategy {
    /// Always try the first available credential.
    FillFirst,
    /// Rotate through credentials round-robin.
    RoundRobin,
    /// Pick a random available credential.
    Random,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PooledCredential {
    pub id: String,
    pub api_key: String,
    pub priority: u32,
    pub source: String,
    #[serde(skip)]
    exhausted_until: Option<Instant>,
}

impl PooledCredential {
    pub fn new(id: &str, api_key: &str, priority: u32, source: &str) -> Self {
        Self {
            id: id.to_string(),
            api_key: api_key.to_string(),
            priority,
            source: source.to_string(),
            exhausted_until: None,
        }
    }

    pub fn is_available(&self, now: Instant) -> bool {
        match self.exhausted_until {
            Some(until) => now >= until,
            None => true,
        }
    }

    pub fn mark_exhausted(&mut self, cooldown: Duration, now: Instant) {
        self.exhausted_until = Some(now + cooldown);
    }
}

/// Thread-safe credential pool for a single provider.
pub struct CredentialPool {
    provider: String,
    entries: Vec<PooledCredential>,
    strategy: PoolStrategy,
    cooldown: Duration,
    round_robin_index: usize,
}

impl CredentialPool {
    pub fn new(provider: &str, strategy: PoolStrategy) -> Self {
        Self {
            provider: provider.to_string(),
            entries: Vec::new(),
            strategy,
            cooldown: Duration::from_secs(3600),
            round_robin_index: 0,
        }
    }

    pub fn with_cooldown(mut self, cooldown: Duration) -> Self {
        self.cooldown = cooldown;
        self
    }

    pub fn add_credential(&mut self, cred: PooledCredential) {
        self.entries.push(cred);
        self.entries.sort_by_key(|c| c.priority);
    }

    pub fn has_available(&self, now: Instant) -> bool {
        self.entries.iter().any(|c| c.is_available(now))
    }

    /// Get the next available credential according to the strategy.
    pub fn next(&mut self, now: Instant) -> Option<&PooledCredential> {
        let available: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_available(now))
            .map(|(i, _)| i)
            .collect();

        if available.is_empty() {
            return None;
        }

        let idx = match self.strategy {
            PoolStrategy::FillFirst => available[0],
            PoolStrategy::RoundRobin => {
                let pos = self.round_robin_index % available.len();
                self.round_robin_index = self.round_robin_index.wrapping_add(1);
                available[pos]
            }
            PoolStrategy::Random => {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                Instant::now().hash(&mut h);
                available[(h.finish() as usize) % available.len()]
            }
        };

        Some(&self.entries[idx])
    }

    /// Mark the current credential as exhausted (called on 401/429/5xx).
    pub fn mark_exhausted(&mut self, id: &str, now: Instant) {
        if let Some(entry) = self.entries.iter_mut().find(|c| c.id == id) {
            entry.mark_exhausted(self.cooldown, now);
        }
    }

    pub fn reset_all(&mut self) {
        for entry in &mut self.entries {
            entry.exhausted_until = None;
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// Registry of credential pools keyed by provider name.
pub struct CredentialRegistry {
    pools: HashMap<String, CredentialPool>,
}

impl CredentialRegistry {
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
        }
    }

    pub fn register(&mut self, pool: CredentialPool) {
        self.pools.insert(pool.provider().to_string(), pool);
    }

    pub fn get(&self, provider: &str) -> Option<&CredentialPool> {
        self.pools.get(provider)
    }

    pub fn get_mut(&mut self, provider: &str) -> Option<&mut CredentialPool> {
        self.pools.get_mut(provider)
    }
}

impl Default for CredentialRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fill_first_strategy() {
        let mut pool = CredentialPool::new("test", PoolStrategy::FillFirst);
        pool.add_credential(PooledCredential::new("k1", "key-a", 0, "env"));
        pool.add_credential(PooledCredential::new("k2", "key-b", 1, "env"));

        let now = Instant::now();
        let c = pool.next(now).unwrap();
        assert_eq!(c.id, "k1"); // lowest priority first
    }

    #[test]
    fn test_exhaustion_failover() {
        let mut pool = CredentialPool::new("test", PoolStrategy::FillFirst)
            .with_cooldown(Duration::from_secs(1));
        pool.add_credential(PooledCredential::new("k1", "key-a", 0, "env"));
        pool.add_credential(PooledCredential::new("k2", "key-b", 1, "env"));

        let now = Instant::now();
        pool.mark_exhausted("k1", now);

        let c = pool.next(now).unwrap();
        assert_eq!(c.id, "k2"); // k1 exhausted, fell back to k2
    }

    #[test]
    fn test_exhaustion_expires() {
        let mut pool = CredentialPool::new("test", PoolStrategy::FillFirst)
            .with_cooldown(Duration::from_millis(1));
        pool.add_credential(PooledCredential::new("k1", "key-a", 0, "env"));

        let now = Instant::now();
        pool.mark_exhausted("k1", now);
        assert!(pool.next(now).is_none()); // all exhausted

        std::thread::sleep(Duration::from_millis(2));
        assert!(pool.next(Instant::now()).is_some()); // cooldown expired
    }
}
