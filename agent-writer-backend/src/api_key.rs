const KEYRING_SERVICE: &str = "agent-writer";
const DEFAULT_PROVIDER: &str = "openai";

fn normalized_provider(raw: &str) -> String {
    let mut safe = String::new();
    for ch in raw.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            safe.push(ch);
        }
    }
    if safe.is_empty() {
        DEFAULT_PROVIDER.to_string()
    } else {
        safe
    }
}

fn normalize_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        Err("API key cannot be empty.".to_string())
    } else {
        Ok(key.to_string())
    }
}

fn credential_dir() -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .map(|path| {
                std::path::PathBuf::from(path)
                    .join("agent-writer")
                    .join("credentials")
            })
            .map_err(|_| "APPDATA is not set; cannot save API key fallback.".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(|path| {
                std::path::PathBuf::from(path)
                    .join(".config")
                    .join("agent-writer")
                    .join("credentials")
            })
            .map_err(|_| "HOME is not set; cannot save API key fallback.".to_string())
    }
}

fn fallback_key_path(provider: &str) -> Result<std::path::PathBuf, String> {
    Ok(credential_dir()?.join(format!("{}.key", normalized_provider(provider))))
}

fn load_api_key_from_keychain(provider: &str) -> Option<String> {
    let provider = normalized_provider(provider);
    let entry = keyring::Entry::new(KEYRING_SERVICE, &provider).ok()?;
    entry
        .get_password()
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn load_api_key_from_fallback(provider: &str) -> Option<String> {
    std::fs::read_to_string(fallback_key_path(provider).ok()?)
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

#[cfg(test)]
fn load_api_key_from_fallback_path(provider: &str, root: std::path::PathBuf) -> Option<String> {
    let path = root.join(format!("{}.key", normalized_provider(provider)));
    std::fs::read_to_string(path)
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn write_api_key_fallback(provider: &str, key: &str, keyring_error: &str) -> Result<(), String> {
    write_api_key_fallback_path(provider, key, keyring_error, credential_dir()?)
}

fn write_api_key_fallback_path(
    provider: &str,
    key: &str,
    keyring_error: &str,
    root: std::path::PathBuf,
) -> Result<(), String> {
    let path = root.join(format!("{}.key", normalized_provider(provider)));
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid API key fallback path.".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| {
        format!("Set error: {keyring_error}; fallback directory failed: {error}")
    })?;
    std::fs::write(&path, key.as_bytes())
        .map_err(|error| format!("Set error: {keyring_error}; fallback write failed: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::warn!(
        "API key stored in local fallback file because keyring was unavailable: {keyring_error}"
    );
    Ok(())
}

fn remove_api_key_fallback(provider: &str) {
    if let Ok(path) = fallback_key_path(provider) {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub(crate) fn store_api_key(provider: &str, key: &str) -> Result<(), String> {
    let provider = normalized_provider(provider);
    let key = normalize_key(key)?;
    let keyring_result = keyring::Entry::new(KEYRING_SERVICE, &provider)
        .map_err(|error| format!("Keyring error: {error}"))
        .and_then(|entry| {
            entry
                .set_password(&key)
                .map_err(|error| format!("Set error: {error}"))
        });

    match keyring_result {
        Ok(()) => {
            if load_api_key_from_keychain(&provider).as_deref() == Some(key.as_str()) {
                remove_api_key_fallback(&provider);
                Ok(())
            } else {
                write_api_key_fallback(&provider, &key, "keyring readback failed after save")
            }
        }
        Err(error) => write_api_key_fallback(&provider, &key, &error),
    }
}

pub(crate) fn resolve_api_key_for_provider(provider: &str) -> Option<String> {
    load_api_key_from_keychain(provider)
        .or_else(|| load_api_key_from_fallback(provider))
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

pub(crate) fn has_api_key_for_provider(provider: &str) -> bool {
    resolve_api_key_for_provider(provider).is_some()
}

pub(crate) fn resolve_api_key() -> Option<String> {
    resolve_api_key_for_provider(DEFAULT_PROVIDER)
}

pub(crate) fn require_api_key() -> Result<String, String> {
    resolve_api_key().ok_or_else(|| "API key not set. Go to Settings.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_provider_falls_back_to_default() {
        assert_eq!(normalized_provider(""), DEFAULT_PROVIDER);
        assert_eq!(normalized_provider("OpenAI"), "openai");
        assert_eq!(normalized_provider("open router!!"), "openrouter");
    }

    #[test]
    fn normalize_key_rejects_empty_values() {
        assert!(normalize_key("   ").is_err());
        assert_eq!(normalize_key("  sk-test  ").unwrap(), "sk-test");
    }

    #[test]
    fn fallback_key_roundtrip_trims_and_normalizes_provider() {
        let root = std::env::temp_dir().join(format!(
            "forge-api-key-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_api_key_fallback_path("Open AI!!", "  sk-fallback-test  ", "test", root.clone())
            .unwrap();
        assert_eq!(
            load_api_key_from_fallback_path("openai", root.clone()).as_deref(),
            Some("sk-fallback-test")
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
