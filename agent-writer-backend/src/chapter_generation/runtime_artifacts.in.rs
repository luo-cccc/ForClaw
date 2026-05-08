#[derive(Debug, Clone)]
pub struct PersistedChapterRuntimeArtifacts {
    pub artifact_refs: Vec<String>,
}

pub fn persist_chapter_runtime_artifacts(
    project: &dyn ChapterGenerationProject,
    request_id: &str,
    context: &BuiltChapterContext,
    settlement_delta: &ChapterSettlementDelta,
    length_telemetry: &ChapterLengthTelemetry,
    generated_content: &str,
) -> Result<PersistedChapterRuntimeArtifacts, String> {
    let project_dir = project.project_data_dir();
    let runtime_dir = project_dir.join("chapter_runtime");
    std::fs::create_dir_all(&runtime_dir).map_err(|e| e.to_string())?;

    let stem = format!(
        "{}-{}",
        context
            .target
            .number
            .map(|number| format!("chapter-{:04}", number))
            .unwrap_or_else(|| "chapter-unknown".to_string()),
        request_id
    );

    let intent_path = runtime_dir.join(format!("{}.intent.json", stem));
    let evidence_path = runtime_dir.join(format!("{}.evidence.json", stem));
    let rule_stack_path = runtime_dir.join(format!("{}.rule_stack.json", stem));
    let trace_path = runtime_dir.join(format!("{}.trace.json", stem));
    let scene_plan_path = runtime_dir.join(format!("{}.scene_plan.json", stem));
    let settlement_path = runtime_dir.join(format!("{}.settlement.json", stem));
    let length_path = runtime_dir.join(format!("{}.length.json", stem));
    let compiled_input_path = runtime_dir.join(format!("{}.compiled_input.json", stem));

    write_json_file(&intent_path, &context.intent_artifact)?;
    write_json_file(&evidence_path, &context.selected_evidence)?;
    write_json_file(&rule_stack_path, &context.rule_stack)?;
    write_json_file(&trace_path, &context.trace_artifact)?;
    write_json_file(&scene_plan_path, &context.scene_plan)?;
    let craft_plan_path = if let Some(ref craft_plan) = context.craft_plan {
        let path = runtime_dir.join(format!("{}.craft_plan.json", stem));
        write_json_file(&path, craft_plan)?;
        Some(path)
    } else {
        None
    };
    write_json_file(&settlement_path, settlement_delta)?;
    write_json_file(&length_path, length_telemetry)?;
    if let Some(ref compiled_input) = context.compiled_input {
        write_json_file(&compiled_input_path, compiled_input)?;
    }

    let replay = SettlementReplay {
        input_content_hash: {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(generated_content.as_bytes());
            format!("{:x}", hasher.finalize())
        },
        memory_snapshot_id: format!("{}", context.target.number.map_or(0, |n| n as i64)),
        output_delta_hash: {
            let json = serde_json::to_string(settlement_delta).unwrap_or_default();
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(json.as_bytes());
            format!("{:x}", hasher.finalize())
        },
        created_at_ms: crate::agent_runtime::now_ms(),
    };
    let replay_path = runtime_dir.join(format!("{}.replay.json", stem));
    write_json_file(&replay_path, &replay)?;

    let mut artifact_refs = vec![
        path_ref(project_dir, &intent_path),
        path_ref(project_dir, &evidence_path),
        path_ref(project_dir, &rule_stack_path),
        path_ref(project_dir, &trace_path),
        path_ref(project_dir, &scene_plan_path),
        path_ref(project_dir, &settlement_path),
        path_ref(project_dir, &length_path),
        path_ref(project_dir, &replay_path),
    ];
    if context.compiled_input.is_some() {
        artifact_refs.push(path_ref(project_dir, &compiled_input_path));
    }
    if let Some(ref craft_plan_path) = craft_plan_path {
        artifact_refs.push(path_ref(project_dir, craft_plan_path));
    }

    Ok(PersistedChapterRuntimeArtifacts {
        artifact_refs,
    })
}

fn write_json_file(path: &std::path::Path, value: &impl serde::Serialize) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

fn path_ref(project_dir: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(project_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
