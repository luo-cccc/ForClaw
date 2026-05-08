/// 通用 Harness 运行时配置 — 解耦具体业务
#[derive(Debug, Clone)]
pub struct HarnessConfig {
    /// 基础 System Prompt 模板
    pub base_system_prompt: String,
    /// 技能萃取 Prompt 模板 (包含 {transcript} 占位符)
    pub extraction_prompt_template: String,
    /// 上下文滑动窗口最大 Token 数
    pub max_context_chars: usize,
    /// 对话记忆最大轮数
    pub max_memory_rounds: usize,
    /// 向量检索 Top K
    pub vector_top_k: usize,
    /// LLM 请求超时 (秒)
    pub request_timeout_secs: u64,
    /// 缓存策略 — 默认禁用后台 keepalive
    pub cache_keepalive_enabled: bool,
    pub extended_cache_enabled: bool,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            base_system_prompt: String::new(),
            extraction_prompt_template: String::from(
                "You are a reflection engine. Analyze the recent interaction transcript and extract 1-2 reusable rules. Output JSON: {\"skills\": [{\"skill\": \"...\", \"category\": \"general\"}]}.",
            ),
            max_context_chars: 2000,
            max_memory_rounds: 20,
            vector_top_k: 5,
            request_timeout_secs: 60,
            cache_keepalive_enabled: false,
            extended_cache_enabled: false,
        }
    }
}
