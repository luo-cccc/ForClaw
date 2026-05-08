#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSideEffectLevel {
    None,
    Read,
    ProviderCall,
    Write,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStage {
    Observe,
    Plan,
    Context,
    Execute,
    Reflect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_type: String,
    pub output_type: String,
    pub side_effect_level: ToolSideEffectLevel,
    pub requires_approval: bool,
    pub timeout_ms: u64,
    pub context_cost_chars: usize,
    pub tags: Vec<String>,
    pub stage: ToolStage,
    pub source: String,
    pub supported_intents: Vec<Intent>,
    pub enabled_by_default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

impl ToolDescriptor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: &str,
        description: &str,
        input_type: &str,
        output_type: &str,
        side_effect_level: ToolSideEffectLevel,
        requires_approval: bool,
        timeout_ms: u64,
        context_cost_chars: usize,
        stage: ToolStage,
    ) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            input_type: input_type.to_string(),
            output_type: output_type.to_string(),
            side_effect_level,
            requires_approval,
            timeout_ms,
            context_cost_chars,
            tags: Vec::new(),
            stage,
            source: "core".to_string(),
            supported_intents: Vec::new(),
            enabled_by_default: true,
            input_schema: None,
        }
    }

    pub fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|tag| tag.to_string()).collect();
        self
    }

    pub fn with_source(mut self, source: &str) -> Self {
        self.source = source.to_string();
        self
    }

    pub fn with_supported_intents(mut self, intents: &[Intent]) -> Self {
        self.supported_intents = intents.to_vec();
        self
    }

    pub fn with_input_schema(mut self, schema: serde_json::Value) -> Self {
        self.input_schema = Some(schema);
        self
    }

    pub fn disabled_by_default(mut self) -> Self {
        self.enabled_by_default = false;
        self
    }

    pub fn supports_intent(&self, intent: &Intent) -> bool {
        self.supported_intents.is_empty() || self.supported_intents.contains(intent)
    }

    /// Convert to OpenAI function calling schema.
    /// Returns None if input_schema is absent (not all tools are LLM-callable).
    pub fn to_openai_tool(&self) -> Option<serde_json::Value> {
        let schema = self.input_schema.as_ref()?;
        Some(serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": schema
            }
        }))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolFilter {
    pub intent: Option<Intent>,
    pub include_requires_approval: bool,
    pub include_disabled: bool,
    pub max_side_effect_level: Option<ToolSideEffectLevel>,
    pub required_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveToolStatus {
    Allowed,
    Disabled,
    IntentMismatch,
    SideEffectTooHigh,
    MissingTag,
    ApprovalRequired,
    PermissionDenied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveToolEntry {
    pub descriptor: ToolDescriptor,
    pub allowed: bool,
    pub status: EffectiveToolStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveToolInventory {
    pub allowed: Vec<ToolDescriptor>,
    pub blocked: Vec<EffectiveToolEntry>,
}

impl EffectiveToolInventory {
    pub fn allowed_names(&self) -> Vec<String> {
        self.allowed.iter().map(|tool| tool.name.clone()).collect()
    }

    pub fn blocked_names(&self) -> Vec<String> {
        self.blocked
            .iter()
            .map(|entry| entry.descriptor.name.clone())
            .collect()
    }

    pub fn to_openai_tools(&self) -> Vec<serde_json::Value> {
        self.allowed
            .iter()
            .filter_map(|tool| tool.to_openai_tool())
            .collect()
    }

    pub fn openai_callable_allowed_count(&self) -> usize {
        self.allowed
            .iter()
            .filter(|tool| tool.input_schema.is_some())
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRegistryError {
    DuplicateTool(String),
}

#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    tools: Vec<ToolDescriptor>,
    index: HashMap<String, usize>,
    generation: u64,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn register(&mut self, descriptor: ToolDescriptor) -> Result<(), ToolRegistryError> {
        if self.index.contains_key(&descriptor.name) {
            return Err(ToolRegistryError::DuplicateTool(descriptor.name));
        }
        self.index.insert(descriptor.name.clone(), self.tools.len());
        self.tools.push(descriptor);
        self.generation += 1;
        Ok(())
    }

    pub fn upsert(&mut self, descriptor: ToolDescriptor) {
        if let Some(index) = self.index.get(&descriptor.name).copied() {
            self.tools[index] = descriptor;
        } else {
            self.index.insert(descriptor.name.clone(), self.tools.len());
            self.tools.push(descriptor);
        }
        self.generation += 1;
    }

    pub fn get(&self, name: &str) -> Option<&ToolDescriptor> {
        self.index
            .get(name)
            .and_then(|index| self.tools.get(*index))
    }

    pub fn list(&self) -> Vec<ToolDescriptor> {
        self.tools.clone()
    }

    pub fn filter(&self, filter: &ToolFilter) -> Vec<ToolDescriptor> {
        self.tools
            .iter()
            .filter(|tool| filter.include_disabled || tool.enabled_by_default)
            .filter(|tool| filter.include_requires_approval || !tool.requires_approval)
            .filter(|tool| {
                filter
                    .max_side_effect_level
                    .map(|level| tool.side_effect_level <= level)
                    .unwrap_or(true)
            })
            .filter(|tool| {
                filter
                    .intent
                    .as_ref()
                    .map(|intent| tool.supports_intent(intent))
                    .unwrap_or(true)
            })
            .filter(|tool| {
                filter
                    .required_tags
                    .iter()
                    .all(|tag| tool.tags.iter().any(|candidate| candidate == tag))
            })
            .cloned()
            .collect()
    }

    pub fn effective_inventory(
        &self,
        filter: &ToolFilter,
        policy: &PermissionPolicy,
    ) -> EffectiveToolInventory {
        let mut inventory = EffectiveToolInventory::default();

        for tool in &self.tools {
            if let Some((status, reason)) = filter_block_reason(tool, filter) {
                inventory.blocked.push(EffectiveToolEntry {
                    descriptor: tool.clone(),
                    allowed: false,
                    status,
                    reason: Some(reason),
                });
                continue;
            }

            match policy.authorize(&tool.name, tool.side_effect_level, tool.requires_approval) {
                PermissionDecision::Allow => inventory.allowed.push(tool.clone()),
                PermissionDecision::Ask { reason } => inventory.blocked.push(EffectiveToolEntry {
                    descriptor: tool.clone(),
                    allowed: false,
                    status: EffectiveToolStatus::ApprovalRequired,
                    reason: Some(reason),
                }),
                PermissionDecision::Deny { reason } => inventory.blocked.push(EffectiveToolEntry {
                    descriptor: tool.clone(),
                    allowed: false,
                    status: EffectiveToolStatus::PermissionDenied,
                    reason: Some(reason),
                }),
            }
        }

        inventory
    }

    pub fn to_effective_openai_tools(
        &self,
        filter: &ToolFilter,
        policy: &PermissionPolicy,
    ) -> Vec<serde_json::Value> {
        self.effective_inventory(filter, policy).to_openai_tools()
    }

    /// Export all tools matching the filter as OpenAI function calling schemas.
    pub fn to_openai_tools(&self, filter: &ToolFilter) -> Vec<serde_json::Value> {
        self.filter(filter)
            .iter()
            .filter_map(|d| d.to_openai_tool())
            .collect()
    }
}
