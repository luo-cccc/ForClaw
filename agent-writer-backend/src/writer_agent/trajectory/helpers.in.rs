fn summarize_run_event(data: &Value) -> String {
    let run_event_type = string_field(data, &["eventType", "event_type"]).unwrap_or("writer.event");
    let payload = data.get("data").unwrap_or(&Value::Null);
    let task_id = string_field(data, &["taskId", "task_id"]);
    let headline = if let Some(message) = string_field(payload, &["message"]) {
        message.to_string()
    } else if let Some(code) = string_field(payload, &["code"]) {
        code.to_string()
    } else if let Some(decision) = string_field(payload, &["decision"]) {
        format!("decision={}", decision)
    } else if let Some(summary) = string_field(payload, &["summary"]) {
        summary.to_string()
    } else {
        compact_json_snippet(payload, 500)
    };
    format!(
        "Run event: {}{} - {}",
        run_event_type,
        optional_labeled(" task=", task_id),
        headline
    )
}

fn string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())
}

fn number_field(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}

fn number_or_float_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        value
            .as_u64()
            .map(|number| number.to_string())
            .or_else(|| value.as_i64().map(|number| number.to_string()))
            .or_else(|| value.as_f64().map(|number| format!("{:.2}", number)))
    })
}

fn optional_labeled(label: &str, value: Option<&str>) -> String {
    value
        .map(|value| format!("{}{}", label, snippet(value, 220)))
        .unwrap_or_default()
}

fn compact_json_snippet(value: &Value, max_chars: usize) -> String {
    serde_json::to_string(value)
        .map(|text| snippet(&text, max_chars))
        .unwrap_or_default()
}

fn snippet(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

fn iso_timestamp(ts_ms: u64) -> String {
    let secs = (ts_ms / 1_000) as i64;
    let nanos = ((ts_ms % 1_000) * 1_000_000) as u32;
    let datetime = DateTime::<Utc>::from_timestamp(secs, nanos).unwrap_or_else(|| {
        DateTime::<Utc>::from_timestamp(0, 0).expect("unix epoch timestamp is valid")
    });
    datetime.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn stable_uuid(session_id: &str, kind: &str, seq: u64) -> String {
    let input = format!("{}::{}::{}", session_id, kind, seq);
    let first = stable_hash64(input.as_bytes(), 0xcbf29ce484222325);
    let second = stable_hash64(input.as_bytes(), 0x84222325cbf29ce4);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (first >> 32) as u32,
        ((first >> 16) & 0xffff) as u16,
        (first & 0xffff) as u16,
        ((second >> 48) & 0xffff) as u16,
        second & 0x0000_ffff_ffff_ffff
    )
}

fn stable_hash64(bytes: &[u8], seed: u64) -> u64 {
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = seed;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[allow(dead_code)]
fn _assert_trace_types(
    _observation: &WriterObservationTrace,
    _task_packet: &WriterTaskPacketTrace,
    _proposal: &WriterProposalTrace,
    _feedback: &WriterFeedbackTrace,
    _lifecycle: &WriterOperationLifecycleTrace,
    _run_event: &WriterRunEvent,
    _recall: &ContextRecallSummary,
    _metacognition: &WriterMetacognitiveSnapshot,
) {
}
