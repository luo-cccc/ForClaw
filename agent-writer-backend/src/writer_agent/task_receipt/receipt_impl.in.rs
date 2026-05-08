impl WriterTaskReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_id: impl Into<String>,
        task_kind: impl Into<String>,
        chapter: Option<String>,
        objective: impl Into<String>,
        required_evidence: Vec<String>,
        expected_artifacts: Vec<String>,
        must_not: Vec<String>,
        source_refs: Vec<String>,
        base_revision: Option<String>,
        created_at_ms: u64,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            task_kind: task_kind.into(),
            chapter,
            objective: objective.into(),
            required_evidence: normalize_strings(required_evidence),
            expected_artifacts: normalize_strings(expected_artifacts),
            must_not: normalize_strings(must_not),
            source_refs: normalize_strings(source_refs),
            base_revision,
            created_at_ms,
        }
    }

    pub fn source_has_evidence(&self, evidence: &str) -> bool {
        let evidence = evidence.trim();
        !evidence.is_empty()
            && self.source_refs.iter().any(|source| {
                source == evidence
                    || source
                        .split_once(':')
                        .map(|(source_type, _)| source_type == evidence)
                        .unwrap_or(false)
            })
    }

    pub fn validate_write_attempt(
        &self,
        task_id: &str,
        chapter: &str,
        base_revision: &str,
        artifact: &str,
    ) -> Vec<WriterTaskReceiptMismatch> {
        let mut mismatches = Vec::new();
        if self.task_id != task_id {
            mismatches.push(WriterTaskReceiptMismatch::new(
                "task_id",
                self.task_id.clone(),
                task_id.to_string(),
                self.task_id.clone(),
            ));
        }
        if self.task_kind != "ChapterGeneration" {
            mismatches.push(WriterTaskReceiptMismatch::new(
                "task_kind",
                "ChapterGeneration",
                self.task_kind.clone(),
                self.task_id.clone(),
            ));
        }
        if self.chapter.as_deref() != Some(chapter) {
            mismatches.push(WriterTaskReceiptMismatch::new(
                "chapter",
                self.chapter.clone().unwrap_or_default(),
                chapter.to_string(),
                self.task_id.clone(),
            ));
        }
        if self.base_revision.as_deref() != Some(base_revision) {
            mismatches.push(WriterTaskReceiptMismatch::new(
                "base_revision",
                self.base_revision.clone().unwrap_or_default(),
                base_revision.to_string(),
                self.task_id.clone(),
            ));
        }
        if !self
            .expected_artifacts
            .iter()
            .any(|expected| expected == artifact)
        {
            mismatches.push(WriterTaskReceiptMismatch::new(
                "expected_artifacts",
                artifact.to_string(),
                self.expected_artifacts.join(","),
                self.task_id.clone(),
            ));
        }
        for evidence in &self.required_evidence {
            if !self.source_has_evidence(evidence) {
                mismatches.push(WriterTaskReceiptMismatch::new(
                    "required_evidence",
                    evidence.clone(),
                    "missing".to_string(),
                    self.task_id.clone(),
                ));
            }
        }
        mismatches
    }

    pub fn validate_artifact_attempt(
        &self,
        task_id: &str,
        artifact: &str,
    ) -> Vec<WriterTaskReceiptMismatch> {
        let mut mismatches = Vec::new();
        if self.task_id != task_id {
            mismatches.push(WriterTaskReceiptMismatch::new(
                "task_id",
                self.task_id.clone(),
                task_id.to_string(),
                self.task_id.clone(),
            ));
        }
        if !self
            .expected_artifacts
            .iter()
            .any(|expected| expected == artifact)
        {
            mismatches.push(WriterTaskReceiptMismatch::new(
                "expected_artifacts",
                artifact.to_string(),
                self.expected_artifacts.join(","),
                self.task_id.clone(),
            ));
        }
        if self.must_not.iter().any(|rule| rule == artifact) {
            mismatches.push(WriterTaskReceiptMismatch::new(
                "must_not",
                format!("not:{}", artifact),
                artifact.to_string(),
                self.task_id.clone(),
            ));
        }
        for evidence in &self.required_evidence {
            if !self.source_has_evidence(evidence) {
                mismatches.push(WriterTaskReceiptMismatch::new(
                    "required_evidence",
                    evidence.clone(),
                    "missing".to_string(),
                    self.task_id.clone(),
                ));
            }
        }
        mismatches
    }
}
