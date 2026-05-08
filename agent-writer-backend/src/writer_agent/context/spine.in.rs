/// Five-layer cache-aware context spine.
///
/// Layers are ordered from most cache-stable (prefix) to most volatile (suffix).
/// The order is: FrozenPrefix → ProjectStablePrefix → FocusPack → HotBuffer → EphemeralScratch.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSpineLayer {
    /// System writing protocol, tool boundaries, output format, North Star constraints.
    /// Must remain byte-stable for the same task/profile.
    FrozenPrefix,
    /// Story Contract, long-term Canon/Author Voice short summaries.
    /// Only refreshed after author-approved long-term setting changes.
    ProjectStablePrefix,
    /// Current chapter Mission, Project Brain chunks, Story Impact, Reader Compensation.
    /// Rebuilt on focus node switch.
    FocusPack,
    /// Current user instruction, selected text, cursor prefix/suffix, recent feedback.
    HotBuffer,
    /// Tool trial logs, temporary reasoning artifacts, failure diagnostic details.
    /// Default: does not enter stable prefix.
    EphemeralScratch,
}

impl ContextSpineLayer {
    pub fn is_stable(&self) -> bool {
        matches!(self, Self::FrozenPrefix | Self::ProjectStablePrefix)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::FrozenPrefix => "FrozenPrefix",
            Self::ProjectStablePrefix => "ProjectStablePrefix",
            Self::FocusPack => "FocusPack",
            Self::HotBuffer => "HotBuffer",
            Self::EphemeralScratch => "EphemeralScratch",
        }
    }
}

fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSpine {
    pub layers: Vec<(ContextSpineLayer, Vec<ContextExcerpt>)>,
    pub frozen_fingerprint: u64,
    pub stable_fingerprint: u64,
    pub focus_fingerprint: u64,
    pub assembled_at: u64,
    pub layer_order: Vec<ContextSpineLayer>,
}

impl ContextSpine {
    pub fn from_pack(pack: &WritingContextPack) -> Self {
        let mut frozen = Vec::new();
        let mut project_stable = Vec::new();
        let mut focus = Vec::new();
        let mut hot = Vec::new();
        let mut ephemeral = Vec::new();

        for source in &pack.sources {
            let layer = classify_source(&source.source);
            match layer {
                ContextSpineLayer::FrozenPrefix => frozen.push(source.clone()),
                ContextSpineLayer::ProjectStablePrefix => project_stable.push(source.clone()),
                ContextSpineLayer::FocusPack => focus.push(source.clone()),
                ContextSpineLayer::HotBuffer => hot.push(source.clone()),
                ContextSpineLayer::EphemeralScratch => ephemeral.push(source.clone()),
            }
        }

        let frozen_fingerprint = layer_fingerprint(&frozen);
        let stable_fingerprint =
            frozen_fingerprint ^ layer_fingerprint(&project_stable);
        let focus_fingerprint =
            stable_fingerprint ^ layer_fingerprint(&focus);

        let layer_order = vec![
            ContextSpineLayer::FrozenPrefix,
            ContextSpineLayer::ProjectStablePrefix,
            ContextSpineLayer::FocusPack,
            ContextSpineLayer::HotBuffer,
            ContextSpineLayer::EphemeralScratch,
        ];

        let layers = vec![
            (ContextSpineLayer::FrozenPrefix, frozen),
            (ContextSpineLayer::ProjectStablePrefix, project_stable),
            (ContextSpineLayer::FocusPack, focus),
            (ContextSpineLayer::HotBuffer, hot),
            (ContextSpineLayer::EphemeralScratch, ephemeral),
        ];

        Self {
            layers,
            frozen_fingerprint,
            stable_fingerprint,
            focus_fingerprint,
            assembled_at: now_ms(),
            layer_order,
        }
    }

    pub fn render_stable_prefix(&self) -> String {
        self.layers
            .iter()
            .filter(|(layer, _)| layer.is_stable())
            .flat_map(|(_, sources)| sources.iter())
            .map(|s| s.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn render_all(&self) -> String {
        self.layers
            .iter()
            .flat_map(|(_, sources)| sources.iter())
            .map(|s| s.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn prefix_char_count(&self) -> usize {
        self.layers
            .iter()
            .filter(|(layer, _)| layer.is_stable())
            .flat_map(|(_, sources)| sources.iter())
            .map(|s| s.char_count)
            .sum()
    }

    pub fn total_char_count(&self) -> usize {
        self.layers
            .iter()
            .flat_map(|(_, sources)| sources.iter())
            .map(|s| s.char_count)
            .sum()
    }

    pub fn estimated_prefix_tokens(&self) -> u64 {
        (self.prefix_char_count() as u64).saturating_div(3)
    }

    pub fn estimated_dynamic_tokens(&self) -> u64 {
        let total = self.total_char_count();
        let prefix = self.prefix_char_count();
        ((total.saturating_sub(prefix)) as u64).saturating_div(3)
    }

    /// Rebuild focus pack and hot buffer while keeping frozen/stable prefix intact.
    pub fn rebuild_focus(
        &mut self,
        new_focus: Vec<ContextExcerpt>,
        new_hot: Vec<ContextExcerpt>,
    ) {
        self.layers.retain(|(layer, _)| layer.is_stable());
        self.layers
            .push((ContextSpineLayer::FocusPack, new_focus.clone()));
        self.layers
            .push((ContextSpineLayer::HotBuffer, new_hot));
        self.layers
            .push((ContextSpineLayer::EphemeralScratch, Vec::new()));
        self.focus_fingerprint = self.stable_fingerprint ^ layer_fingerprint(&new_focus);
        self.assembled_at = now_ms();
    }

    /// Build cache stability report: why this spine may or may not hit the cache.
    pub fn build_stability_report(&self, previous: Option<&ContextSpine>) -> CacheStabilityReport {
        let mut miss_reasons = Vec::new();
        let mut prefix_churn_sources = Vec::new();

        if let Some(prev) = previous {
            if prev.frozen_fingerprint != self.frozen_fingerprint {
                miss_reasons.push("FrozenPrefix changed — system prompt or tool schema modified".to_string());
                prefix_churn_sources.push("FrozenPrefix".to_string());
            }
            if prev.stable_fingerprint != self.stable_fingerprint {
                miss_reasons.push(
                    "ProjectStablePrefix refreshed — Story Contract or long-term settings changed"
                        .to_string(),
                );
                prefix_churn_sources.push("ProjectStablePrefix".to_string());
            }
            if prev.focus_fingerprint != self.focus_fingerprint {
                miss_reasons.push("FocusPack changed — chapter or focus node switched".to_string());
            }
        } else {
            miss_reasons.push("First call — no cache baseline".to_string());
        }

        let dynamic_tokens = self.estimated_dynamic_tokens();
        if dynamic_tokens > 8000 {
            miss_reasons.push(format!(
                "Dynamic tail unusually large ({} est. tokens) — may reduce cache efficiency",
                dynamic_tokens
            ));
        }

        CacheStabilityReport {
            miss_reasons,
            prefix_churn_sources,
            estimated_prefix_tokens: self.estimated_prefix_tokens(),
            estimated_dynamic_tokens: dynamic_tokens,
            frozen_fingerprint: self.frozen_fingerprint,
            stable_fingerprint: self.stable_fingerprint,
            focus_fingerprint: self.focus_fingerprint,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStabilityReport {
    pub miss_reasons: Vec<String>,
    pub prefix_churn_sources: Vec<String>,
    pub estimated_prefix_tokens: u64,
    pub estimated_dynamic_tokens: u64,
    pub frozen_fingerprint: u64,
    pub stable_fingerprint: u64,
    pub focus_fingerprint: u64,
}

impl CacheStabilityReport {
    pub fn likely_cache_hit(&self) -> bool {
        self.miss_reasons.is_empty()
            || (self.miss_reasons.len() == 1
                && self.miss_reasons[0].contains("First call"))
    }
}

/// Classify a context source into the appropriate spine layer.
fn classify_source(source: &ContextSource) -> ContextSpineLayer {
    match source {
        ContextSource::SystemContract => ContextSpineLayer::FrozenPrefix,
        ContextSource::ProjectBrief
        | ContextSource::BookState
        | ContextSource::AuthorStyle
        | ContextSource::CanonSlice
        | ContextSource::PromiseSlice
        | ContextSource::DecisionSlice => ContextSpineLayer::ProjectStablePrefix,
        ContextSource::ChapterMission
        | ContextSource::ArcSnapshot
        | ContextSource::VolumeSnapshot
        | ContextSource::NextBeat
        | ContextSource::ResultFeedback
        | ContextSource::StoryImpactRadius
        | ContextSource::ReaderCompensation
        | ContextSource::OutlineSlice
        | ContextSource::RagExcerpt
        | ContextSource::PreviousChapter
        | ContextSource::NextChapter => ContextSpineLayer::FocusPack,
        ContextSource::CursorPrefix
        | ContextSource::CursorSuffix
        | ContextSource::SelectedText
        | ContextSource::NeighborText => ContextSpineLayer::HotBuffer,
    }
}

fn layer_fingerprint(sources: &[ContextExcerpt]) -> u64 {
    let mut combined = String::new();
    for s in sources {
        combined.push_str(&format!(
            "{:?}:{}:{}:",
            s.source, s.char_count, s.content
        ));
    }
    fnv1a_hash(combined.as_bytes())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Focus node types — writing-domain anchor points for FocusPack assembly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusNodeKind {
    Chapter,
    Scene,
    CanonEntity,
    Promise,
    EmotionalDebt,
    ResearchSource,
    AuthorVoiceSample,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusState {
    pub active_node_kind: FocusNodeKind,
    pub active_node_id: String,
    pub active_chapter: Option<String>,
    pub active_scene: Option<String>,
    pub related_entity_ids: Vec<String>,
    pub related_promise_ids: Vec<String>,
    pub last_switched_at: u64,
}

impl Default for FocusState {
    fn default() -> Self {
        Self {
            active_node_kind: FocusNodeKind::Chapter,
            active_node_id: String::new(),
            active_chapter: None,
            active_scene: None,
            related_entity_ids: Vec::new(),
            related_promise_ids: Vec::new(),
            last_switched_at: 0,
        }
    }
}

impl FocusState {
    pub fn switch_to(&mut self, kind: FocusNodeKind, node_id: &str) -> bool {
        let changed = self.active_node_kind != kind || self.active_node_id != node_id;
        if changed {
            self.active_node_kind = kind;
            self.active_node_id = node_id.to_string();
            self.last_switched_at = now_ms();
        }
        changed
    }

    pub fn set_chapter(&mut self, chapter: Option<&str>, scene: Option<&str>) {
        self.active_chapter = chapter.map(|s| s.to_string());
        self.active_scene = scene.map(|s| s.to_string());
    }

    pub fn focus_fingerprint(&self) -> u64 {
        let mut data = format!("{:?}:{}", self.active_node_kind, self.active_node_id);
        if let Some(ref ch) = self.active_chapter {
            data.push_str(&format!(":ch={}", ch));
        }
        fnv1a_hash(data.as_bytes())
    }
}
