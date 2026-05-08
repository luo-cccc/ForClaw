#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderCompensationReviewChain {
    pub target_reader_lack: String,
    pub protagonist_proxy: String,
    pub relationship_soil: Vec<String>,
    pub pressure_scene: String,
    pub interest_mechanism: String,
    pub payoff_point: String,
    pub payoff_path: String,
    pub next_lack: String,
    pub active_debts: Vec<super::memory::EmotionalDebtSummary>,
    pub risks: Vec<String>,
}

impl WriterAgentKernel {
    pub fn reader_compensation_review_chain(&self) -> ReaderCompensationReviewChain {
        let mut review = ReaderCompensationReviewChain {
            target_reader_lack: String::new(),
            protagonist_proxy: String::new(),
            relationship_soil: Vec::new(),
            pressure_scene: String::new(),
            interest_mechanism: String::new(),
            payoff_point: String::new(),
            payoff_path: String::new(),
            next_lack: String::new(),
            active_debts: Vec::new(),
            risks: Vec::new(),
        };

        if let Ok(Some(profile)) =
            self.memory
                .get_reader_compensation_profile(&self.project_id)
        {
            review.target_reader_lack = profile.primary_lack;
            review.protagonist_proxy = profile.protagonist_proxy_state;
            if !profile.dominant_relationship_soil.trim().is_empty() {
                review.relationship_soil =
                    vec![profile.dominant_relationship_soil.clone()];
            }
        }

        if let Some(ref chapter) = self.active_chapter {
            if let Ok(Some(mission)) =
                self.memory
                    .get_chapter_mission(&self.project_id, chapter)
            {
                review.pressure_scene = mission.pressure_scene;
                review.interest_mechanism = mission.interest_mechanism;
                review.payoff_point = mission.payoff_target;
                review.payoff_path = mission.payoff_path;
                review.next_lack = mission.next_lack_opened;
                if !mission.relationship_soil_this_chapter.trim().is_empty()
                    && !review.relationship_soil.contains(&mission.relationship_soil_this_chapter)
                {
                    review
                        .relationship_soil
                        .push(mission.relationship_soil_this_chapter);
                }
            }
        }

        if let Ok(debts) = self
            .memory
            .get_open_emotional_debts(&self.project_id)
        {
            review.active_debts = debts;
        }

        if review.active_debts.is_empty() && review.payoff_point.trim().is_empty() {
            review.risks.push(
                "无活跃情绪债务且本章无补偿目标 — 读者可能缺少情感投入理由"
                    .to_string(),
            );
        }

        for debt in &review.active_debts {
            if debt.overdue_risk == "high" {
                review.risks.push(format!(
                    "情绪债务 '{}' 面临过期风险 — 建议在本章或近期兑现",
                    debt.title
                ));
            }
        }

        if review.pressure_scene.trim().is_empty()
            && !review.payoff_point.trim().is_empty()
        {
            review.risks.push(
                "本章设置了补偿目标但缺少压迫场景 — 补偿可能缺乏力度"
                    .to_string(),
            );
        }

        if review.next_lack.trim().is_empty()
            && !review.payoff_point.trim().is_empty()
        {
            review
                .risks
                .push("本章补偿后未打开新的读者缺口 — 追读可能断裂".to_string());
        }

        review
    }
}
