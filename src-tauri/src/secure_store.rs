const SERVICE: &str = "ideall";
const MAX_KEY_LEN: usize = 160;
const MAX_VALUE_LEN: usize = 512 * 1024;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureStoreStatus {
    pub backend: &'static str,
    pub native: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureStoreSelfTestResult {
    pub backend: &'static str,
    pub round_trip: bool,
    pub cleaned_up: bool,
}

fn validate_key(key: &str) -> Result<&str, String> {
    let k = key.trim();
    if k.is_empty() || k.len() > MAX_KEY_LEN {
        return Err("invalid-key".into());
    }
    if !k
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-'))
    {
        return Err("invalid-key".into());
    }
    Ok(k)
}

fn validate_value(value: &str) -> Result<(), String> {
    if value.len() > MAX_VALUE_LEN {
        return Err("value-too-large".into());
    }
    Ok(())
}

/// Linux 默认走 Secret Service（`linux-native-sync-persistent`）。
/// WSL / 无 gnome-keyring 的环境里 DBus 名不可激活，任何 get/set/delete 都会失败，
/// 进而让 0.2 data-epoch 清凭据卡死在 BootGate。探测失败时回退到会话级 keyutils。
#[cfg(target_os = "linux")]
fn ensure_linux_credential_backend() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        use keyring::credential::CredentialApi;
        let secret_service_usable = keyring::secret_service::SsCredential::new_with_target(
            None,
            SERVICE,
            "ideall:backend-probe",
        )
        .and_then(|credential| match credential.get_password() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error),
        })
        .is_ok();
        if secret_service_usable {
            return;
        }
        eprintln!(
            "[ideall] Secret Service unavailable; falling back to linux keyutils for credentials"
        );
        keyring::set_default_credential_builder(keyring::keyutils::default_credential_builder());
    });
}

fn entry(key: &str) -> Result<keyring::Entry, String> {
    #[cfg(target_os = "linux")]
    ensure_linux_credential_backend();
    let k = validate_key(key)?;
    keyring::Entry::new(SERVICE, k).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn secure_store_status() -> SecureStoreStatus {
    #[cfg(target_os = "linux")]
    ensure_linux_credential_backend();
    SecureStoreStatus {
        backend: "system-keychain",
        native: true,
    }
}

#[tauri::command]
pub fn secure_store_get(key: String) -> Result<Option<String>, String> {
    match entry(&key)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub fn secure_store_set(key: String, value: String) -> Result<(), String> {
    validate_value(&value)?;
    entry(&key)?.set_password(&value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn secure_store_delete(key: String) -> Result<(), String> {
    match entry(&key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// 在真实系统凭据库中写入一次性随机值，读回校验后立即删除。
/// 该命令不接收前端 secret，也不返回测试 key/value，适合作为安装后验收入口。
#[tauri::command]
pub fn secure_store_self_test() -> Result<SecureStoreSelfTestResult, String> {
    let key = format!("ideall:self-test:{}", uuid::Uuid::new_v4().simple());
    let value = uuid::Uuid::new_v4().to_string();
    let credential = entry(&key)?;
    credential
        .set_password(&value)
        .map_err(|error| format!("write-failed: {error}"))?;

    let round_trip = matches!(credential.get_password(), Ok(ref actual) if actual == &value);
    let delete_result = credential.delete_credential();
    let cleaned_up = matches!(delete_result, Ok(()) | Err(keyring::Error::NoEntry))
        && matches!(credential.get_password(), Err(keyring::Error::NoEntry));

    if !round_trip || !cleaned_up {
        let _ = credential.delete_credential();
        return Err(if !round_trip {
            "round-trip-failed".into()
        } else {
            "cleanup-failed".into()
        });
    }

    Ok(SecureStoreSelfTestResult {
        backend: "system-keychain",
        round_trip,
        cleaned_up,
    })
}

#[cfg(test)]
mod tests {
    use super::validate_key;

    #[test]
    fn key_validation_allows_namespaced_safe_keys() {
        assert_eq!(
            validate_key("ideall:agent.settings-api_key_1").unwrap(),
            "ideall:agent.settings-api_key_1"
        );
        assert!(validate_key("../secret").is_err());
        assert!(validate_key("").is_err());
    }
}
