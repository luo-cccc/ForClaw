impl WriterAgentKernel {
    pub fn add_feedback_event(&mut self, action: crate::writer_agent::feedback::FeedbackAction) {
        let event = crate::writer_agent::feedback::ProposalFeedback {
            proposal_id: format!("eval-{}", super::now_ms()),
            action,
            final_text: None,
            reason: None,
            created_at: super::now_ms(),
        };
        self.feedback_events.push(event);
    }

    pub fn story_snapshot(&self) -> StorySnapshot {
        StorySnapshot {
            character_count: self.memory.list_characters(None).unwrap_or_default().len(),
            protagonist_name: self
                .memory
                .list_characters(Some("protagonist"))
                .unwrap_or_default()
                .first()
                .map(|c| c.name.clone()),
            open_promise_count: self.memory.get_open_promise_summaries().unwrap_or_default().len(),
            total_chapters: self
                .memory
                .list_recent_chapter_results(&self.project_id, 100)
                .unwrap_or_default()
                .len(),
            latest_reveal: String::new(),
        }
    }

    pub fn recent_session_summary(&self, count: usize) -> SessionSummary {
        let results = self
            .memory
            .list_recent_chapter_results(&self.project_id, count)
            .unwrap_or_default();
        SessionSummary {
            chapters_written: results.len(),
            total_words: results.iter().map(|r| r.summary.len()).sum(),
            promises_advanced: 0,
            new_characters: 0,
        }
    }

    pub fn today_five_summary(&self) -> TodayFiveSummary {
        let character_count = self.memory.list_characters(None).unwrap_or_default().len();
        let recent_results = self.memory.list_recent_chapter_results(&self.project_id, 100).unwrap_or_default();
        let has_no_content = character_count == 0 && recent_results.is_empty();

        if has_no_content {
            return TodayFiveSummary {
                chapter_title: self.active_chapter.clone(),
                is_onboarding: true,
                items: vec![
                    TodayFiveItem {
                        slot: "guard".to_string(),
                        label: "欢迎使用 Forge".to_string(),
                        value: "欢迎使用 Forge".to_string(),
                        detail: "欢迎使用 Forge".to_string(),
                        tone: "✅ 一切正常".to_string(),
                    },
                    TodayFiveItem {
                        slot: "contract".to_string(),
                        label: "我是你的写作伙伴".to_string(),
                        value: "我是你的写作伙伴".to_string(),
                        detail: "我是你的写作伙伴".to_string(),
                        tone: "✅ 一切正常".to_string(),
                    },
                    TodayFiveItem {
                        slot: "mission".to_string(),
                        label: "先写一个开头".to_string(),
                        value: "先写一个开头".to_string(),
                        detail: "先写一个开头".to_string(),
                        tone: "✅ 一切正常".to_string(),
                    },
                    TodayFiveItem {
                        slot: "promise".to_string(),
                        label: "然后点'生成下一章'".to_string(),
                        value: "然后点'生成下一章'".to_string(),
                        detail: "然后点'生成下一章'".to_string(),
                        tone: "✅ 一切正常".to_string(),
                    },
                    TodayFiveItem {
                        slot: "next".to_string(),
                        label: "我会帮你记住角色、线索和承诺".to_string(),
                        value: "我会帮你记住角色、线索和承诺".to_string(),
                        detail: "我会帮你记住角色、线索和承诺".to_string(),
                        tone: "✅ 一切正常".to_string(),
                    },
                ],
            };
        }

        let ledger = self.ledger_snapshot();
        let debt = self.story_debt_snapshot();
        let trace = self.trace_snapshot(20);
        let current_chapter = self.active_chapter.clone();
        let ranked_by_pressure = {
            let mut sorted = ledger.open_promises.clone();
            let ch = current_chapter.as_deref().unwrap_or("Chapter-1");
            sorted.sort_by(|a, b| {
                let pa = crate::writer_agent::promise_planner::promise_subject_pressure(
                    a,
                    &self.memory,
                    ch,
                );
                let pb = crate::writer_agent::promise_planner::promise_subject_pressure(
                    b,
                    &self.memory,
                    ch,
                );
                pb.partial_cmp(&pa)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            sorted
        };
        let contract_debt = debt
            .entries
            .iter()
            .find(|entry| entry.category == StoryDebtCategory::StoryContract);
        let mission_debt = debt
            .entries
            .iter()
            .find(|entry| entry.category == StoryDebtCategory::ChapterMission);
        let canon_risk = debt.entries.iter().find(|entry| {
            matches!(
                entry.category,
                StoryDebtCategory::CanonRisk | StoryDebtCategory::TimelineRisk
            )
        });
        let open_promise = ranked_by_pressure.first();
        let next_beat = ledger.next_beat.as_ref();
        let latest_result = current_chapter
            .as_deref()
            .and_then(|chapter| {
                ledger
                    .recent_chapter_results
                    .iter()
                    .find(|result| result.chapter_title == chapter)
            })
            .or_else(|| ledger.recent_chapter_results.first());
        let chapter_mission = ledger.active_chapter_mission.as_ref().or_else(|| {
            current_chapter.as_deref().and_then(|chapter| {
                ledger
                    .chapter_missions
                    .iter()
                    .find(|mission| mission.chapter_title == chapter)
            })
        });
        // Try scene-level objective first
        let scene_objective = current_chapter.as_deref().and_then(|ch| {
            self.memory.list_scenes_by_chapter(ch).ok().and_then(|scenes| {
                scenes.first().and_then(|s| {
                    self.memory.get_scene_state(s.id).ok().flatten().map(|state| state.objective)
                })
            })
        });
        let next_value = scene_objective
            .filter(|obj| !obj.trim().is_empty())
            .unwrap_or_else(|| next_beat.map(|beat| beat.goal.clone())
                .or_else(|| latest_result.map(|r| r.summary.clone()))
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| "No next move".to_string()));
        let guard_value = if !debt.entries.is_empty() {
            format!("{} active guards", debt.open_count)
        } else if trace
            .task_packets
            .first()
            .is_some_and(|packet| packet.foundation_complete)
        {
            "aligned".to_string()
        } else {
            "quiet".to_string()
        };
        let guard_detail = if !debt.entries.is_empty() {
            debt.entries
                .first()
                .map(|entry| entry.title.clone())
                .unwrap_or_else(|| "Story debt needs attention.".to_string())
        } else {
            "No active story debt surfaced.".to_string()
        };
        let character_count = self
            .memory
            .list_characters(None)
            .unwrap_or_default()
            .len();
        let active_relationship_count = current_chapter.as_deref().map_or(0, |ch| {
            self.memory
                .list_characters(None)
                .unwrap_or_default()
                .iter()
                .filter_map(|c| self.memory.get_active_relationships(c.id, ch).ok())
                .flatten()
                .count()
        });
        let guard_detail = format!(
            "{} characters, {} relationships. {}",
            character_count, active_relationship_count, guard_detail
        );
        let concealed_count = self.memory
            .list_knowledge_items(Some("objective"))
            .unwrap_or_default()
            .len();
        let guard_detail = format!(
            "{} | {} concealed truths tracked",
            guard_detail, concealed_count
        );

        let time_context = current_chapter
            .as_deref()
            .and_then(|ch| {
                self.memory
                    .get_time_mapping_for_chapter(ch)
                    .ok()
                    .and_then(|mappings| {
                        mappings.first().and_then(|m| {
                            if m.narrative_mode == "present" {
                                return None;
                            }
                            self.memory.list_time_slices().ok().and_then(|slices| {
                                slices.iter().find(|ts| ts.id == m.time_slice_id).map(|ts| {
                                    format!(
                                        " | 故事时间: {} ({})",
                                        ts.label, m.narrative_mode
                                    )
                                })
                            })
                        })
                    })
            })
            .unwrap_or_default();
        let guard_detail = format!("{}{}", guard_detail, time_context);
        let chapter_summary = format!(
            "{}章 · {}条线索 · {}个角色",
            current_chapter.as_deref().unwrap_or("?"),
            ledger.open_promises.len(),
            self.memory.list_characters(None).unwrap_or_default().len(),
        );
        let guard_detail = format!("{} | {}", guard_detail, chapter_summary);
        // Emotional debt tracking: check if any open promise relates to emotional keywords
        let emotional_keywords = ["愤怒", "悲伤", "背叛", "恐惧", "失去", "悔恨", "自责", "绝望", "压抑", "痛苦"];
        let has_emotional_cues = ledger
            .open_promises
            .iter()
            .any(|p| emotional_keywords.iter().any(|kw| p.title.contains(kw) || p.description.contains(kw)));
        let guard_detail = if has_emotional_cues {
            format!("{} | 情绪跟踪: 已激活", guard_detail)
        } else {
            guard_detail
        };

        // Session retrospective summary
        let session = self.recent_session_summary(5);
        let guard_detail = format!(
            "{} | 本次写作: 写了{}章, 推进{}条线索",
            guard_detail, session.chapters_written, session.promises_advanced
        );

        // Trust-building feedback stats
        let total_feedback = self.feedback_events.len();
        let guard_detail = if total_feedback > 0 {
            let accepted_count = self
                .feedback_events
                .iter()
                .filter(|f| matches!(f.action, crate::writer_agent::feedback::FeedbackAction::Accepted))
                .count();
            let ignored_count = self
                .feedback_events
                .iter()
                .filter(|f| matches!(f.action, crate::writer_agent::feedback::FeedbackAction::Snoozed))
                .count();
            let accept_pct = if total_feedback > 0 {
                (accepted_count * 100) / total_feedback
            } else {
                0
            };
            let ignore_pct = if total_feedback > 0 {
                (ignored_count * 100) / total_feedback
            } else {
                0
            };
            format!(
                "{} | 你的写作习惯: 接受建议 {}%, 忽略提醒 {}%",
                guard_detail, accept_pct, ignore_pct
            )
        } else {
            guard_detail
        };

        // Author burnout check
        let guard_detail = if let Ok(results) = self.memory.list_recent_chapter_results(&self.project_id, 5) {
            if results.len() >= 3 {
                let recent_summaries_short = results.iter().rev().take(2).all(|r| r.summary.len() < 50);
                if recent_summaries_short {
                    format!("{}\n💡 最近几章的摘要较短——可能是连续写作导致的疲劳。建议休息一下或回顾前几章。", guard_detail)
                } else {
                    guard_detail
                }
            } else {
                guard_detail
            }
        } else {
            guard_detail
        };

        TodayFiveSummary {
            chapter_title: current_chapter.clone(),
            is_onboarding: false,
            items: vec![
                TodayFiveItem {
                    slot: "guard".to_string(),
                    label: "今日状态".to_string(),
                    value: guard_value,
                    detail: guard_detail,
                    tone: if debt.canon_risk_count > 0 || debt.mission_count > 0 {
                        "⚠️ 需要注意".to_string()
                    } else if debt.open_count > 0 {
                        "📝 提个醒".to_string()
                    } else {
                        "✅ 一切正常".to_string()
                    },
                },
                TodayFiveItem {
                    slot: "contract".to_string(),
                    label: "全书承诺".to_string(),
                    value: contract_debt
                        .map(|entry| entry.title.clone())
                        .or_else(|| {
                            ledger
                                .story_contract
                                .as_ref()
                                .map(|contract| contract.reader_promise.clone())
                        })
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "No story contract".to_string()),
                    detail: contract_debt
                        .map(|entry| entry.message.clone())
                        .or_else(|| {
                            ledger
                                .story_contract
                                .as_ref()
                                .map(|contract| contract.main_conflict.clone())
                        })
                        .unwrap_or_else(|| "Set the book-level promise.".to_string()),
                    tone: if contract_debt.is_some() {
                        "⚠️ 需要注意".to_string()
                    } else {
                        "📝 提个醒".to_string()
                    },
                },
                TodayFiveItem {
                    slot: "mission".to_string(),
                    label: "本章目标".to_string(),
                    value: mission_debt
                        .map(|entry| entry.title.clone())
                        .or_else(|| chapter_mission.map(|mission| mission.mission.clone()))
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "No chapter mission".to_string()),
                    detail: mission_debt
                        .map(|entry| entry.message.clone())
                        .or_else(|| {
                            chapter_mission.map(|mission| mission.expected_ending.clone())
                        })
                        .unwrap_or_else(|| "Open a chapter mission.".to_string()),
                    tone: if mission_debt.is_some() {
                        "⚠️ 需要注意".to_string()
                    } else {
                        "📝 提个醒".to_string()
                    },
                },
                TodayFiveItem {
                    slot: "promise".to_string(),
                    label: "待兑现线索".to_string(),
                    value: open_promise
                        .map(|promise| promise.title.clone())
                        .unwrap_or_else(|| "No open promise".to_string()),
                    detail: open_promise
                        .map(|p| {
                            let char_name = p
                                .related_entities
                                .iter()
                                .find_map(|e| e.strip_prefix("character:"))
                                .and_then(|name| {
                                    self.memory.get_character_by_name(name).ok().flatten()
                                })
                                .map(|c| c.name);
                            let mut base = match char_name {
                                Some(name) => format!(
                                    "{} → {} ({} 的承诺)",
                                    p.description, p.expected_payoff, name
                                ),
                                None => format!("{} → {}", p.description, p.expected_payoff),
                            };
                            let concealed = self.memory
                                .list_knowledge_items(Some("objective"))
                                .unwrap_or_default();
                            let relates_to_concealed = concealed.iter().any(|ki| {
                                p.description.contains(&ki.topic) || p.title.contains(&ki.topic)
                            });
                            if relates_to_concealed {
                                base.push_str(" ⚠️ 还不能揭");
                            }
                            base
                        })
                        .unwrap_or_else(|| "No open promise".to_string()),
                    tone: if open_promise.is_some() {
                        "📝 提个醒".to_string()
                    } else {
                        "✅ 一切正常".to_string()
                    },
                },
                TodayFiveItem {
                    slot: "next".to_string(),
                    label: "下一步".to_string(),
                    value: next_value,
                    detail: {
                        let mut base = canon_risk
                            .map(|entry| entry.message.clone())
                            .or_else(|| latest_result.and_then(|result| result.new_conflicts.first().cloned()))
                            .unwrap_or_else(|| "No immediate blocker.".to_string());
                        let reader_beat = latest_result.as_ref().and_then(|result| {
                            let changes = result.state_changes.join(" ");
                            if changes.contains("冲突") {
                                Some("紧张")
                            } else if changes.contains("和解") {
                                Some("感动")
                            } else {
                                None
                            }
                        });
                        let reader_expectation = open_promise
                            .and_then(|p| if p.expected_payoff.is_empty() { None } else { Some(p.expected_payoff.as_str()) });
                        if reader_beat.is_some() || reader_expectation.is_some() {
                            base.push_str("。读者期待：");
                            if let Some(beat) = reader_beat {
                                base.push_str(beat);
                                if reader_expectation.is_some() {
                                    base.push('，');
                                }
                            }
                            if let Some(expectation) = reader_expectation {
                                base.push_str(expectation);
                            }
                        }
                        base
                    },
                    tone: if canon_risk.is_some() {
                        "⚠️ 需要注意".to_string()
                    } else {
                        "📝 提个醒".to_string()
                    },
                },
            ],
        }
    }
}
