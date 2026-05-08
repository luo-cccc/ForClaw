use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Events emitted by the editor that ambient agents can subscribe to.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum EditorEvent {
    #[serde(rename = "cursor_moved")]
    CursorMoved {
        chapter: String,
        position: usize,
        paragraph: String,
    },
    #[serde(rename = "text_changed")]
    TextChanged {
        chapter: String,
        full_text_snippet: String,
        change_summary: String,
    },
    #[serde(rename = "idle_tick")]
    IdleTick {
        request_id: Option<String>,
        idle_ms: u64,
        chapter: String,
        paragraph: String,
        prefix: String,
        suffix: String,
        cursor_position: usize,
    },
    #[serde(rename = "selection_changed")]
    SelectionChanged {
        from: usize,
        to: usize,
        text: String,
        chapter: String,
    },
    #[serde(rename = "chapter_saved")]
    ChapterSaved {
        chapter: String,
        content_length: usize,
        revision: String,
    },
    #[serde(rename = "chapter_switched")]
    ChapterSwitched { from: Option<String>, to: String },
    #[serde(rename = "session_ended")]
    SessionEnded,
    #[serde(rename = "keyword_detected")]
    KeywordDetected {
        keywords: Vec<String>,
        chapter: String,
        paragraph: String,
    },
}

/// Result of an ambient agent's processing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "output_kind")]
pub enum AgentOutput {
    #[serde(rename = "ghost_text")]
    GhostText {
        request_id: Option<String>,
        text: String,
        position: usize,
    },
    #[serde(rename = "multi_ghost")]
    MultiGhost {
        request_id: Option<String>,
        position: usize,
        intent: String,
        candidates: Vec<GhostCandidate>,
    },
    #[serde(rename = "ghost_end")]
    GhostEnd {
        request_id: Option<String>,
        position: usize,
        reason: String,
    },
    #[serde(rename = "hover_hint")]
    HoverHint {
        message: String,
        from: usize,
        to: usize,
    },
    #[serde(rename = "entity_card")]
    EntityCard {
        keyword: String,
        content: String,
        chapter: String,
    },
    #[serde(rename = "semantic_lint")]
    SemanticLint {
        message: String,
        from: usize,
        to: usize,
        severity: String,
    },
    #[serde(rename = "storyboard_marker")]
    StoryboardMarker {
        chapter: String,
        message: String,
        level: String,
    },
    #[serde(rename = "epiphany")]
    Epiphany { skill: String, category: String },
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GhostCandidate {
    pub id: String,
    pub label: String,
    pub text: String,
}

/// Trait for ambient agents — background daemons that respond to editor events.
/// Each agent runs in its own tokio task and never blocks the main thread.
/// Designed for the "Cursor Co-Pilot" model: agents are silent observers,
/// outputting non-intrusive hints (ghost text, hover, lint, markers).
#[async_trait::async_trait]
pub trait AmbientAgent: Send + Sync {
    fn name(&self) -> &str;
    fn subscribed_events(&self) -> Vec<String>;
    async fn process(&self, event: EditorEvent, cancel: CancellationToken) -> Option<AgentOutput>;
}

/// The event bus routes editor events to subscribed ambient agents.
/// Uses tokio::sync::broadcast for efficient multi-consumer fan-out.
pub struct AmbientEventBus {
    tx: broadcast::Sender<EditorEvent>,
    agents: Vec<AmbientAgentHandle>,
    output: Arc<dyn Fn(AgentOutput) + Send + Sync>,
}

struct AmbientAgentHandle {
    name: String,
    join_handle: Option<tokio::task::JoinHandle<()>>,
    cancel: CancellationToken,
}

impl AmbientEventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            agents: Vec::new(),
            output: Arc::new(|_| {}),
        }
    }

    pub fn set_output_handler(&mut self, output: Arc<dyn Fn(AgentOutput) + Send + Sync>) {
        self.output = output;
    }

    /// Publish an editor event to all subscribers. Non-blocking.
    pub fn publish(
        &self,
        event: EditorEvent,
    ) -> Result<usize, Box<broadcast::error::SendError<EditorEvent>>> {
        self.tx.send(event).map_err(Box::new)
    }

    pub fn spawn_agent<A: AmbientAgent + 'static>(&mut self, agent: Arc<A>) {
        self.spawn(agent, self.output.clone());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EditorEvent> {
        self.tx.subscribe()
    }

    /// Spawn an ambient agent. The agent runs in a background tokio task.
    /// Events are filtered by subscription. Output goes to the callback.
    ///
    /// For "debounce replacement": call abort_agent() before spawning a new
    /// instance of the same agent type when context changes.
    pub fn spawn<A: AmbientAgent + 'static>(
        &mut self,
        agent: Arc<A>,
        on_output: Arc<dyn Fn(AgentOutput) + Send + Sync>,
    ) {
        let mut rx = self.subscribe();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let name = agent.name().to_string();
        let name_for_handle = name.clone();
        let subscribed = agent.subscribed_events();

        let join_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_clone.cancelled() => break,
                    result = rx.recv() => {
                        match result {
                            Ok(event) => {
                                let ek = event_kind(&event);
                                if !subscribed.is_empty() && !subscribed.iter().any(|e| e == ek) {
                                    continue;
                                }
                                if let Some(output) = agent.process(event, cancel_clone.clone()).await {
                                    on_output(output);
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                eprintln!("[ambient] Agent {} lagged by {} events", name, n);
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });

        self.agents.push(AmbientAgentHandle {
            name: name_for_handle,
            join_handle: Some(join_handle),
            cancel,
        });
    }

    /// Abort a specific agent. Also used for debounce: when new text arrives,
    /// abort old CoWriter and spawn a fresh one.
    pub fn abort_agent(&mut self, name: &str) {
        self.agents.retain(|h| {
            if h.name == name {
                h.cancel.cancel();
                false
            } else {
                true
            }
        });
    }

    /// Gracefully shut down all agents.
    pub async fn shutdown(&mut self) {
        for h in &self.agents {
            h.cancel.cancel();
        }
        for mut h in std::mem::take(&mut self.agents) {
            if let Some(jh) = h.join_handle.take() {
                let _ = jh.await;
            }
        }
    }
}

fn event_kind(event: &EditorEvent) -> &str {
    match event {
        EditorEvent::CursorMoved { .. } => "cursor_moved",
        EditorEvent::TextChanged { .. } => "text_changed",
        EditorEvent::IdleTick { .. } => "idle_tick",
        EditorEvent::SelectionChanged { .. } => "selection_changed",
        EditorEvent::ChapterSaved { .. } => "chapter_saved",
        EditorEvent::ChapterSwitched { .. } => "chapter_switched",
        EditorEvent::SessionEnded => "session_ended",
        EditorEvent::KeywordDetected { .. } => "keyword_detected",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TestAgent {
        name: String,
        events: Vec<String>,
    }

    #[async_trait]
    impl AmbientAgent for TestAgent {
        fn name(&self) -> &str {
            &self.name
        }
        fn subscribed_events(&self) -> Vec<String> {
            self.events.clone()
        }
        async fn process(&self, _: EditorEvent, _: CancellationToken) -> Option<AgentOutput> {
            Some(AgentOutput::None)
        }
    }

    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = AmbientEventBus::new(16);
        let mut rx = bus.subscribe();
        bus.publish(EditorEvent::SessionEnded).unwrap();
        assert!(matches!(
            rx.recv().await.unwrap(),
            EditorEvent::SessionEnded
        ));
    }

    #[tokio::test]
    async fn test_agent_receives_subscribed_event() {
        let mut bus = AmbientEventBus::new(16);
        let count = Arc::new(AtomicU32::new(0));
        let c = count.clone();
        bus.spawn(
            Arc::new(TestAgent {
                name: "t".into(),
                events: vec!["idle_tick".into()],
            }),
            Arc::new(move |_| {
                c.fetch_add(1, Ordering::Relaxed);
            }),
        );
        bus.publish(EditorEvent::IdleTick {
            request_id: None,
            idle_ms: 500,
            chapter: "ch1".into(),
            paragraph: "h".into(),
            prefix: "h".into(),
            suffix: String::new(),
            cursor_position: 0,
        })
        .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        bus.shutdown().await;
        assert!(count.load(Ordering::Relaxed) >= 1);
    }

    #[tokio::test]
    async fn test_agent_ignores_unsubscribed() {
        let mut bus = AmbientEventBus::new(16);
        let count = Arc::new(AtomicU32::new(0));
        let c = count.clone();
        bus.spawn(
            Arc::new(TestAgent {
                name: "t".into(),
                events: vec!["chapter_saved".into()],
            }),
            Arc::new(move |_| {
                c.fetch_add(1, Ordering::Relaxed);
            }),
        );
        bus.publish(EditorEvent::CursorMoved {
            chapter: "c".into(),
            position: 0,
            paragraph: "h".into(),
        })
        .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        bus.shutdown().await;
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }
}
