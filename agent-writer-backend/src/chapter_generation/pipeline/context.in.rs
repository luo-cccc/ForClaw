struct ContextComposer {
    max_chars: usize,
    text: String,
    sources: Vec<ChapterContextSource>,
    warnings: Vec<String>,
}

impl ContextComposer {
    fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            text: String::new(),
            sources: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn remaining_chars(&self) -> usize {
        self.max_chars.saturating_sub(char_count(&self.text))
    }

    fn add_source(
        &mut self,
        source_type: &str,
        id: &str,
        label: &str,
        content: &str,
        source_cap: usize,
        score: Option<f32>,
    ) {
        if content.trim().is_empty() || self.remaining_chars() == 0 {
            return;
        }

        let header = format!("## {}\n", label);
        let footer = "\n\n";
        let overhead = char_count(&header) + char_count(footer);
        let remaining = self.remaining_chars();
        if remaining <= overhead {
            self.warnings
                .push(format!("Context budget exhausted before adding {}.", label));
            return;
        }

        let allowed = source_cap.min(remaining - overhead);
        let original_chars = char_count(content);
        let (included, included_chars, truncated) = truncate_text_report(content, allowed);

        self.text.push_str(&header);
        self.text.push_str(&included);
        self.text.push_str(footer);

        if truncated {
            self.warnings.push(format!(
                "{} truncated from {} to {} chars.",
                label, original_chars, included_chars
            ));
        }

        self.sources.push(ChapterContextSource {
            source_type: source_type.to_string(),
            id: id.to_string(),
            label: label.to_string(),
            original_chars,
            included_chars,
            truncated,
            score,
        });
    }

    fn finish(
        self,
    ) -> (
        String,
        Vec<ChapterContextSource>,
        ChapterContextBudgetReport,
    ) {
        let included_chars = char_count(&self.text);
        let truncated_source_count = self
            .sources
            .iter()
            .filter(|source| source.truncated)
            .count();
        let report = ChapterContextBudgetReport {
            max_chars: self.max_chars,
            included_chars,
            source_count: self.sources.len(),
            truncated_source_count,
            warnings: self.warnings,
        };
        (self.text, self.sources, report)
    }
}

fn select_previous_nodes(
    outline: &[storage::OutlineNode],
    target_index: usize,
    max_count: usize,
) -> Vec<&storage::OutlineNode> {
    let start = target_index.saturating_sub(max_count);
    outline[start..target_index].iter().collect()
}

fn select_next_nodes(
    outline: &[storage::OutlineNode],
    target_index: usize,
    max_count: usize,
) -> Vec<&storage::OutlineNode> {
    outline
        .iter()
        .skip(target_index + 1)
        .take(max_count)
        .collect()
}

fn build_adjacent_chapter_context(
    project: &dyn ChapterGenerationProject,
    nodes: Vec<&storage::OutlineNode>,
) -> String {
    if nodes.is_empty() {
        return "None (first chapter or no previous outline nodes).".to_string();
    }

    nodes
        .iter()
        .map(|node| {
            let text = project
                .load_chapter(&node.chapter_title)
                .unwrap_or_default();
            build_previous_chapter_structured_context(node.chapter_title.as_str(), &node.summary, &text)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn build_previous_chapter_structured_context(
    chapter_title: &str,
    summary: &str,
    text: &str,
) -> String {
    let summary_line = compact_line(summary, 140);
    let carryovers = infer_carryovers(summary, text);
    let consequences = infer_consequences(summary, text);
    let unresolved = infer_unresolved_threads(summary, text);
    let closing_image = extract_closing_image(text);

    let mut lines = vec![format!("[{}]", chapter_title)];
    lines.push(format!("Summary: {}", summary_line));
    if !carryovers.is_empty() {
        lines.push(format!("Carryovers: {}", carryovers.join(" / ")));
    }
    if !consequences.is_empty() {
        lines.push(format!("Consequences: {}", consequences.join(" / ")));
    }
    if !unresolved.is_empty() {
        lines.push(format!("Unresolved: {}", unresolved.join(" / ")));
    }
    if let Some(image) = closing_image {
        lines.push(format!("Closing image: {}", image));
    }
    lines.join("\n")
}

fn build_next_chapter_context(nodes: Vec<&storage::OutlineNode>) -> String {
    if nodes.is_empty() {
        return "No next chapter outline node.".to_string();
    }

    nodes
        .iter()
        .map(|node| format!("[{}]\n{}", node.chapter_title, node.summary))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn infer_carryovers(summary: &str, text: &str) -> Vec<String> {
    let mut items = Vec::new();
    if contains_any(summary, &["账册", "半页", "密信", "钥匙", "入口"]) {
        items.push("critical object/entry clue remains active".to_string());
    }
    if contains_any(summary, &["张三", "背叛", "隐瞒", "试探", "误会"]) {
        items.push("trust pressure with Zhang San remains unresolved".to_string());
    }
    if contains_any(text, &["寒影刀", "发烫", "震", "冰凉", "刀鞘"]) {
        items.push("Han Ying blade is actively reacting".to_string());
    }
    items
}

fn infer_consequences(summary: &str, text: &str) -> Vec<String> {
    let mut items = Vec::new();
    if contains_any(summary, &["代价", "只能", "一次", "时限"]) {
        items.push("entry conditions impose a real cost or time limit".to_string());
    }
    if contains_any(summary, &["宗门", "追兵", "找来", "来人"]) || contains_any(text, &["北境宗门"]) {
        items.push("external faction pressure is already closing in".to_string());
    }
    items
}

fn infer_unresolved_threads(summary: &str, text: &str) -> Vec<String> {
    let mut items = Vec::new();
    if contains_any(summary, &["真相", "秘密", "身份", "封门"]) || contains_any(text, &["封门"]) {
        items.push("the sealing truth should move closer but not fully detonate yet".to_string());
    }
    if contains_any(summary, &["旧债", "还债"]) || contains_any(text, &["旧债"]) {
        items.push("the old debt needs pressure or partial payoff, not another empty reminder".to_string());
    }
    items
}

fn extract_closing_image(text: &str) -> Option<String> {
    let last = text
        .split(['。', '！', '？', '!', '?', '\n'])
        .map(str::trim)
        .rfind(|line| !line.is_empty())?;
    Some(compact_line(last, 90))
}

fn compact_line(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for ch in text.chars().take(max_chars) {
        result.push(ch);
    }
    if text.chars().count() > max_chars {
        result.push_str("...");
    }
    result
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn select_lore_entries<'a>(
    entries: &'a [storage::LoreEntry],
    query: &str,
    max_count: usize,
) -> Vec<(f32, &'a storage::LoreEntry)> {
    let mut scored = entries
        .iter()
        .map(|entry| {
            let haystack = format!("{}\n{}", entry.keyword, entry.content);
            (relevance_score(query, &haystack), entry)
        })
        .filter(|(score, _)| *score > 0.0)
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(max_count);
    scored
}

fn select_rag_chunks(
    project: &dyn ChapterGenerationProject,
    query: &str,
    max_count: usize,
) -> Vec<(f32, Vec<String>, agent_harness_core::Chunk)> {
    let db = match VectorDB::load(project.brain_path()) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(
                "Skipping Project Brain chunks because '{}' is unreadable: {}",
                project.brain_path().display(),
                e
            );
            return vec![];
        }
    };

    let scored = db
        .chunks
        .into_iter()
        .map(|chunk| {
            let haystack = format!(
                "{}\n{}\n{}\n{}",
                chunk.chapter,
                chunk.keywords.join("\n"),
                chunk.topic.clone().unwrap_or_default(),
                chunk.text
            );
            (relevance_score(query, &haystack), chunk)
        })
        .filter(|(score, _)| *score > 0.0)
        .collect::<Vec<_>>();
    rerank_text_chunks(scored, query, |chunk| {
        format!(
            "{}\n{}\n{}\n{}",
            chunk.chapter,
            chunk.keywords.join("\n"),
            chunk.topic.clone().unwrap_or_default(),
            chunk.text
        )
    })
    .into_iter()
    .take(max_count)
    .collect()
}

fn relevance_score(query: &str, haystack: &str) -> f32 {
    let haystack = haystack.to_lowercase();
    let mut score = 0f32;
    for needle in query_needles(query) {
        if haystack.contains(&needle.to_lowercase()) {
            score += needle.chars().count().max(1) as f32;
        }
    }
    score
}

fn query_needles(query: &str) -> Vec<String> {
    let mut needles = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_alphanumeric() || is_cjk(ch) {
            current.push(ch);
        } else if !current.is_empty() {
            push_needle(&mut needles, &current);
            current.clear();
        }
    }
    if !current.is_empty() {
        push_needle(&mut needles, &current);
    }
    needles.truncate(64);
    needles
}

fn push_needle(needles: &mut Vec<String>, token: &str) {
    if char_count(token) >= 2 {
        needles.push(token.to_string());
    }

    let chars = token.chars().collect::<Vec<_>>();
    if chars.len() >= 4 && chars.iter().any(|ch| is_cjk(*ch)) {
        for window in chars.windows(2).take(16) {
            needles.push(window.iter().collect());
        }
    }
}

fn is_cjk(ch: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&ch)
        || ('\u{3400}'..='\u{4DBF}').contains(&ch)
        || ('\u{F900}'..='\u{FAFF}').contains(&ch)
}
