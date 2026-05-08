#[tauri::command]
pub fn set_api_key(provider: String, key: String) -> Result<(), String> {
    crate::api_key::store_api_key(&provider, &key)
}

#[tauri::command]
pub fn check_api_key(provider: String) -> Result<bool, String> {
    Ok(crate::api_key::has_api_key_for_provider(&provider))
}
