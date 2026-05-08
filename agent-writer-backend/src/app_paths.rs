pub(crate) fn safe_filename_component(raw: &str) -> String {
    let mut safe = String::new();
    let mut last_was_dash = false;
    for ch in raw.trim().to_lowercase().chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch)
        } else if ch == ' ' || ch == '-' || ch == '_' {
            Some('-')
        } else {
            None
        };

        if let Some(ch) = next {
            if ch == '-' {
                if last_was_dash {
                    continue;
                }
                last_was_dash = true;
            } else {
                last_was_dash = false;
            }
            safe.push(ch);
        }
    }
    let safe = safe.trim_matches('-');
    if safe.is_empty() {
        "default".to_string()
    } else {
        safe.to_string()
    }
}

pub(crate) fn log_dir() -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .map(|p| {
                std::path::PathBuf::from(p)
                    .join("agent-writer")
                    .join("logs")
            })
            .map_err(|_| "APPDATA not set".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::home_dir()
            .map(|p| p.join(".config").join("agent-writer").join("logs"))
            .ok_or_else(|| "Home dir not found".to_string())
    }
}
