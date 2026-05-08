use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const APPROX_TABLE_COUNT: usize = 4;
const APPROX_BITS_PER_TABLE: usize = 16;
const APPROX_MIN_CANDIDATES_MULTIPLIER: usize = 8;

fn jieba() -> &'static jieba_rs::Jieba {
    static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    JIEBA.get_or_init(jieba_rs::Jieba::new)
}

/// Tokenize text for BM25. Uses jieba for Chinese segmentation,
/// whitespace as fallback. Min token length: 2 chars.
fn tokenize(text: &str) -> Vec<String> {
    let is_cjk = text.chars().any(|c| c as u32 > 0x2E80);
    if is_cjk {
        jieba()
            .cut(text, true)
            .into_iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| s.chars().count() >= 2)
            .map(|s| s.to_string())
            .collect()
    } else {
        text.split_whitespace()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| s.len() >= 2)
            .map(|s| s.to_string())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub chapter: String,
    pub text: String,
    pub embedding: Vec<f32>,
    pub keywords: Vec<String>,
    pub topic: Option<String>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub source_revision: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub chunk_index: Option<usize>,
    #[serde(default)]
    pub archived: bool,
}

pub struct VectorDB {
    pub chunks: Vec<Chunk>,
    avg_text_len: f32,
    approx_tables: Vec<HashMap<u64, Vec<usize>>>,
    /// Pre-tokenized texts aligned with `chunks` to avoid repeated
    /// jieba segmentation during search.
    tokenized_texts: Vec<Vec<String>>,
    /// Pre-computed embedding magnitudes to speed up cosine similarity.
    embedding_magnitudes: Vec<f32>,
}

impl Default for VectorDB {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorDB {
    pub fn new() -> Self {
        Self {
            chunks: vec![],
            avg_text_len: 1.0,
            approx_tables: vec![HashMap::new(); APPROX_TABLE_COUNT],
            tokenized_texts: vec![],
            embedding_magnitudes: vec![],
        }
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let chunks: Vec<Chunk> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        let avg_text_len = if chunks.is_empty() {
            1.0
        } else {
            chunks
                .iter()
                .map(|c| tokenize(&c.text).len() as f32)
                .sum::<f32>()
                / chunks.len() as f32
        };
        let mut db = Self {
            chunks,
            avg_text_len,
            approx_tables: vec![HashMap::new(); APPROX_TABLE_COUNT],
            tokenized_texts: vec![],
            embedding_magnitudes: vec![],
        };
        db.rebuild_index();
        Ok(db)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.chunks).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("Write tmp failed: {}", e))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("Atomic rename failed: {}", e))
    }

    pub fn upsert(&mut self, chunk: Chunk) {
        let tokenized = tokenize(&chunk.text);
        let mag = chunk.embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if let Some(idx) = self.chunks.iter().position(|c| c.id == chunk.id) {
            self.chunks[idx] = chunk;
            self.tokenized_texts[idx] = tokenized;
            self.embedding_magnitudes[idx] = mag;
        } else {
            self.chunks.push(chunk);
            self.tokenized_texts.push(tokenized);
            self.embedding_magnitudes.push(mag);
        }
        self.refresh_avg_text_len();
        self.rebuild_index();
    }

    pub fn archive_chapter_revision(&mut self, chapter: &str, active_revision: &str) {
        let mut to_remove = Vec::new();
        for (i, chunk) in self.chunks.iter().enumerate() {
            if chunk.chapter == chapter && chunk.source_revision.as_deref() == Some(active_revision)
            {
                to_remove.push(i);
            }
        }
        for &i in to_remove.iter().rev() {
            self.chunks.remove(i);
            self.tokenized_texts.remove(i);
            self.embedding_magnitudes.remove(i);
        }
        for chunk in &mut self.chunks {
            if chunk.chapter == chapter && chunk.source_revision.as_deref() != Some(active_revision)
            {
                chunk.archived = true;
            }
        }
        self.refresh_avg_text_len();
        self.rebuild_index();
    }

    pub fn remove_chapter(&mut self, chapter: &str) {
        let chapter_source_ref = format!("chapter:{}", chapter);
        let mut to_remove = Vec::new();
        for (i, c) in self.chunks.iter().enumerate() {
            if c.chapter == chapter || c.source_ref.as_deref() == Some(&chapter_source_ref) {
                to_remove.push(i);
            }
        }
        for &i in to_remove.iter().rev() {
            self.chunks.remove(i);
            self.tokenized_texts.remove(i);
            self.embedding_magnitudes.remove(i);
        }
        self.refresh_avg_text_len();
        self.rebuild_index();
    }

    fn refresh_avg_text_len(&mut self) {
        self.avg_text_len = if self.tokenized_texts.is_empty() {
            1.0
        } else {
            self.tokenized_texts
                .iter()
                .map(|t| t.len() as f32)
                .sum::<f32>()
                / self.tokenized_texts.len() as f32
        };
    }

    pub fn rebuild_index(&mut self) {
        self.approx_tables = vec![HashMap::new(); APPROX_TABLE_COUNT];
        self.tokenized_texts = self.chunks.iter().map(|c| tokenize(&c.text)).collect();
        self.embedding_magnitudes = self
            .chunks
            .iter()
            .map(|c| c.embedding.iter().map(|x| x * x).sum::<f32>().sqrt())
            .collect();
        for (idx, chunk) in self.chunks.iter().enumerate() {
            if chunk.embedding.is_empty() {
                continue;
            }
            for table in 0..APPROX_TABLE_COUNT {
                let hash = approx_hash(&chunk.embedding, table);
                self.approx_tables[table].entry(hash).or_default().push(idx);
            }
        }
    }

    // ── BM25 lexical scoring ────────────────────────────────────────
    fn bm25_score_precomputed(
        query_terms: &[String],
        doc_terms: &[String],
        document_frequency: &HashMap<String, usize>,
        doc_count: usize,
        avg_text_len: f32,
    ) -> f32 {
        let doc_len = doc_terms.len() as f32;
        let k1 = 1.5;
        let b = 0.75;

        query_terms
            .iter()
            .map(|term| {
                let tf = doc_terms.iter().filter(|t| *t == term).count() as f32;
                if tf == 0.0 {
                    return 0.0;
                }
                let df = document_frequency.get(term).copied().unwrap_or(0).max(1) as f32;
                let idf = ((doc_count as f32 - df + 0.5) / (df + 0.5)).ln().max(0.0);
                idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * doc_len / avg_text_len))
            })
            .sum()
    }

    // ── Pure cosine (backward compat) ───────────────────────────────
    pub fn search(&self, query_embedding: &[f32], top_k: usize) -> Vec<(f32, &Chunk)> {
        self.search_internal(query_embedding, top_k, false)
    }

    pub fn search_all(&self, query_embedding: &[f32], top_k: usize) -> Vec<(f32, &Chunk)> {
        self.search_internal(query_embedding, top_k, true)
    }

    fn search_internal(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        include_archived: bool,
    ) -> Vec<(f32, &Chunk)> {
        let candidate_ids =
            self.approximate_candidate_ids(query_embedding, top_k, include_archived);
        let query_mag: f32 = query_embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mut scored: Vec<(f32, &Chunk)> = candidate_ids
            .into_iter()
            .filter_map(|idx| {
                let chunk = self.chunks.get(idx)?;
                if !include_archived && chunk.archived {
                    return None;
                }
                let dot: f32 = query_embedding
                    .iter()
                    .zip(chunk.embedding.iter())
                    .map(|(x, y)| x * y)
                    .sum();
                let mag_b = *self.embedding_magnitudes.get(idx)?;
                let sim = if query_mag == 0.0 || mag_b == 0.0 {
                    0.0
                } else {
                    dot / (query_mag * mag_b)
                };
                Some((sim, chunk))
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    // ── Hybrid: Sem(cosine) + Lex(BM25) + Sym(metadata match) ──────
    pub fn search_hybrid(
        &self,
        query: &str,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Vec<(f32, &Chunk)> {
        self.search_hybrid_internal(query, query_embedding, top_k, false)
    }

    pub fn search_hybrid_all(
        &self,
        query: &str,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Vec<(f32, &Chunk)> {
        self.search_hybrid_internal(query, query_embedding, top_k, true)
    }

    fn search_hybrid_internal(
        &self,
        query: &str,
        query_embedding: &[f32],
        top_k: usize,
        include_archived: bool,
    ) -> Vec<(f32, &Chunk)> {
        let query_terms = tokenize(query);
        let query_term_set: HashSet<&str> = query_terms.iter().map(String::as_str).collect();
        let query_lower = query.to_lowercase();
        let mut document_frequency: HashMap<String, usize> =
            query_terms.iter().map(|term| (term.clone(), 0)).collect();
        let mut tokenized_docs: Vec<(usize, &Chunk, &Vec<String>)> = Vec::new();
        let candidate_ids =
            self.approximate_candidate_ids(query_embedding, top_k, include_archived);

        for idx in candidate_ids {
            let Some(chunk) = self.chunks.get(idx) else {
                continue;
            };
            let Some(doc_terms) = self.tokenized_texts.get(idx) else {
                continue;
            };
            if !include_archived && chunk.archived {
                continue;
            }
            let mut seen_terms = HashSet::new();
            for term in doc_terms {
                if query_term_set.contains(term.as_str()) && seen_terms.insert(term.as_str()) {
                    if let Some(count) = document_frequency.get_mut(term) {
                        *count += 1;
                    }
                }
            }
            tokenized_docs.push((idx, chunk, doc_terms));
        }

        let doc_count = tokenized_docs.len().max(1);
        let avg_text_len = self.avg_text_len.max(1.0);

        let query_mag: f32 = query_embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mut scored: Vec<(f32, &Chunk)> = tokenized_docs
            .iter()
            .map(|(idx, chunk, doc_terms)| {
                let dot: f32 = query_embedding
                    .iter()
                    .zip(chunk.embedding.iter())
                    .map(|(x, y)| x * y)
                    .sum();
                let mag_b = self.embedding_magnitudes.get(*idx).copied().unwrap_or(0.0);
                let sem = if query_mag == 0.0 || mag_b == 0.0 {
                    0.0
                } else {
                    dot / (query_mag * mag_b)
                };
                let lex = Self::bm25_score_precomputed(
                    &query_terms,
                    doc_terms,
                    &document_frequency,
                    doc_count,
                    avg_text_len,
                ) * 0.3; // BM25 权重 0.3
                let sym = if chunk
                    .keywords
                    .iter()
                    .any(|kw| query_lower.contains(&kw.to_lowercase()))
                {
                    0.5
                } else {
                    0.0
                };
                (sem + lex + sym, *chunk)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    fn approximate_candidate_ids(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        include_archived: bool,
    ) -> Vec<usize> {
        if query_embedding.is_empty() || self.chunks.len() <= top_k.saturating_mul(4) {
            return self
                .chunks
                .iter()
                .enumerate()
                .filter(|(_, chunk)| include_archived || !chunk.archived)
                .map(|(idx, _)| idx)
                .collect();
        }

        let mut candidate_ids = HashSet::new();
        for table in 0..APPROX_TABLE_COUNT {
            let hash = approx_hash(query_embedding, table);
            if let Some(indices) = self
                .approx_tables
                .get(table)
                .and_then(|buckets| buckets.get(&hash))
            {
                candidate_ids.extend(indices.iter().copied());
            }
        }

        if candidate_ids.len() < top_k.saturating_mul(APPROX_MIN_CANDIDATES_MULTIPLIER) {
            let threshold = top_k
                .saturating_mul(APPROX_MIN_CANDIDATES_MULTIPLIER)
                .max(top_k);
            let query_mag: f32 = query_embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            let mut fallback: Vec<(f32, usize)> = self
                .chunks
                .iter()
                .enumerate()
                .filter(|(_, chunk)| include_archived || !chunk.archived)
                .map(|(idx, chunk)| {
                    let dot: f32 = query_embedding
                        .iter()
                        .zip(chunk.embedding.iter())
                        .map(|(x, y)| x * y)
                        .sum();
                    let mag_b = self.embedding_magnitudes.get(idx).copied().unwrap_or(0.0);
                    let sim = if query_mag == 0.0 || mag_b == 0.0 {
                        0.0
                    } else {
                        dot / (query_mag * mag_b)
                    };
                    (sim, idx)
                })
                .collect();
            if fallback.len() > threshold {
                fallback.select_nth_unstable_by(threshold - 1, |a, b| {
                    b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            fallback.truncate(threshold);
            candidate_ids.extend(fallback.into_iter().map(|(_, idx)| idx));
        }

        if candidate_ids.is_empty() {
            return self
                .chunks
                .iter()
                .enumerate()
                .filter(|(_, chunk)| include_archived || !chunk.archived)
                .map(|(idx, _)| idx)
                .collect();
        }

        let mut ids = candidate_ids.into_iter().collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }
}

fn approx_hash(embedding: &[f32], table: usize) -> u64 {
    let dims = embedding.len().min(64);
    let mut hash = 0u64;
    for bit in 0..APPROX_BITS_PER_TABLE {
        let mut acc = 0.0f32;
        for dim in 0..dims {
            let coeff = if pseudo_sign(table, bit, dim) {
                1.0
            } else {
                -1.0
            };
            acc += embedding[dim] * coeff;
        }
        if acc >= 0.0 {
            hash |= 1u64 << bit;
        }
    }
    hash
}

fn pseudo_sign(table: usize, bit: usize, dim: usize) -> bool {
    let key = ((table as u64) << 48) | ((bit as u64) << 32) | (dim as u64);
    let mut v = key.wrapping_mul(0x9E37_79B1_85EB_CA87);
    v ^= v >> 33;
    v = v.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    v ^= v >> 29;
    (v & 1) == 0
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    dot / (mag_a * mag_b)
}

pub fn extract_keywords(text: &str) -> Vec<String> {
    let stopwords: &[&str] = &[
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
        "do", "does", "did", "will", "would", "could", "should", "may", "might", "can", "shall",
        "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through",
        "during", "before", "after", "above", "below", "between", "and", "but", "or", "nor", "not",
        "so", "yet", "both", "either", "neither", "each", "every", "all", "any", "few", "more",
        "most", "other", "some", "such", "no", "only", "own", "same", "than", "too", "very",
        "just", "because", "about", "over", "under", "again", "further", "then", "once", "here",
        "there", "when", "where", "why", "how", "which", "who", "whom", "this", "that", "these",
        "those", "it", "its", "he", "she", "they", "them", "their", "we", "us", "our", "i", "me",
        "my", "you", "your", "的", "了", "在", "是", "我", "有", "和", "就", "不", "人", "都",
        "一", "一个", "上", "也", "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看",
        "好", "自己", "这", "他", "她", "它", "们",
    ];
    let mut seen = std::collections::HashSet::new();
    tokenize(text)
        .into_iter()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| {
            w.chars().count() >= 2 && !stopwords.contains(&w.as_str()) && seen.insert(w.clone())
        })
        .take(8)
        .collect()
}

pub fn chunk_text(text: &str, max_chars: usize) -> Vec<(String, Vec<String>, Option<String>)> {
    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut chunks = Vec::new();
    let mut current = String::new();

    for para in paragraphs {
        let trimmed = para.trim();
        if trimmed.is_empty() {
            continue;
        }
        if current.len() + trimmed.len() > max_chars && !current.is_empty() {
            let full = current.trim().to_string();
            let keywords = extract_keywords(&full);
            let topic = full.chars().take(30).collect::<String>();
            chunks.push((full, keywords, Some(topic)));
            current = String::new();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(trimmed);
    }

    if !current.trim().is_empty() {
        let full = current.trim().to_string();
        let keywords = extract_keywords(&full);
        let topic = full.chars().take(30).collect::<String>();
        chunks.push((full, keywords, Some(topic)));
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_chunk(id: &str, text: &str, embedding: Vec<f32>) -> Chunk {
        Chunk {
            id: id.to_string(),
            chapter: "Chapter-1".to_string(),
            text: text.to_string(),
            embedding,
            keywords: Vec::new(),
            topic: None,
            source_ref: None,
            source_revision: None,
            source_kind: None,
            chunk_index: None,
            archived: false,
        }
    }

    #[test]
    fn hybrid_search_preserves_bm25_lexical_ranking() {
        let mut db = VectorDB::new();
        db.upsert(test_chunk(
            "unrelated",
            "old door rumor wind repeats in the tavern without the target clue",
            vec![],
        ));
        db.upsert(test_chunk(
            "lexical-target",
            "jade ring payoff jade ring payoff jade ring payoff north sect clue",
            vec![],
        ));
        db.upsert(test_chunk(
            "background",
            "market road lantern quiet scene with no matching promise terms",
            vec![],
        ));

        let results = db.search_hybrid("jade ring payoff", &[], 3);
        let first_id = results.first().map(|(_, chunk)| chunk.id.as_str());

        assert_eq!(first_id, Some("lexical-target"));
    }

    #[test]
    fn rebuild_index_preserves_semantic_search_results() {
        let mut db = VectorDB::new();
        db.upsert(test_chunk("a", "alpha", vec![1.0, 0.0, 0.0]));
        db.upsert(test_chunk("b", "beta", vec![0.0, 1.0, 0.0]));
        db.upsert(test_chunk("c", "gamma", vec![0.0, 0.0, 1.0]));
        db.rebuild_index();

        let results = db.search(&[0.95, 0.05, 0.0], 2);
        assert_eq!(
            results.first().map(|(_, chunk)| chunk.id.as_str()),
            Some("a")
        );
    }
}
