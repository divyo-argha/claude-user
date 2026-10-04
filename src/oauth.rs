use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::profiles;

const OAUTH_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const EXPIRY_BUFFER_MS: i64 = 5 * 60 * 1000; // 5 minutes

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthData {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<i64>,
    #[serde(rename = "refreshTokenExpiresAt")]
    pub refresh_token_expires_at: Option<i64>,
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredentials {
    #[serde(rename = "claudeAiOauth")]
    pub claude_ai_oauth: Option<OAuthData>,
}

#[derive(Debug, Clone)]
pub enum CredentialSource {
    Keychain(String),
    File(std::path::PathBuf),
}

#[derive(Debug, Clone)]
pub struct ProfileCredentials {
    pub data: StoredCredentials,
    pub source: CredentialSource,
}

pub fn get_keychain_service_name(config_dir: &Path) -> String {
    let normalized = config_dir.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let result = hasher.finalize();
    let hex_digest: String = result.iter().map(|b| format!("{b:02x}")).collect();
    let prefix = &hex_digest[..8];
    format!("Claude Code-credentials-{prefix}")
}

pub fn get_keychain_account_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "claude-code-user".to_string())
}

#[cfg(target_os = "macos")]
pub fn read_keychain(service: &str, account: &str) -> Option<String> {
    let output = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-a", account, "-w", "-s", service])
        .output()
        .ok()?;

    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
pub fn read_keychain(_service: &str, _account: &str) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
pub fn write_keychain(service: &str, account: &str, data: &str) -> Result<()> {
    let status = Command::new("/usr/bin/security")
        .args(["add-generic-password", "-U", "-a", account, "-s", service, "-w", data])
        .status()
        .context("failed to execute security command")?;

    if !status.success() {
        return Err(anyhow!("security add-generic-password failed with status {status}"));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn write_keychain(_service: &str, _account: &str, _data: &str) -> Result<()> {
    Ok(())
}

pub fn load_profile_credentials(profile_name: &str) -> Result<Option<ProfileCredentials>> {
    let dir = profiles::profile_dir(profile_name)?;
    let is_current = profiles::current_profile()?.as_deref() == Some(profile_name);
    let account = get_keychain_account_name();

    #[cfg(target_os = "macos")]
    {
        // 1. Try profile-specific hashed keychain service
        let service = get_keychain_service_name(&dir);
        if let Some(content) = read_keychain(&service, &account)
            && let Ok(creds) = serde_json::from_str::<StoredCredentials>(&content)
            && creds.claude_ai_oauth.is_some()
        {
            return Ok(Some(ProfileCredentials {
                data: creds,
                source: CredentialSource::Keychain(service),
            }));
        }

        // 2. If this is the active/current profile, try default "Claude Code-credentials" service
        if is_current {
            let default_service = "Claude Code-credentials";
            if let Some(content) = read_keychain(default_service, &account)
                && let Ok(creds) = serde_json::from_str::<StoredCredentials>(&content)
                && creds.claude_ai_oauth.is_some()
            {
                return Ok(Some(ProfileCredentials {
                    data: creds,
                    source: CredentialSource::Keychain(default_service.to_string()),
                }));
            }
        }
    }

    // 3. Try <profile_dir>/.credentials.json
    let file_path = dir.join(".credentials.json");
    if file_path.exists()
        && let Ok(content) = fs::read_to_string(&file_path)
        && let Ok(creds) = serde_json::from_str::<StoredCredentials>(&content)
        && creds.claude_ai_oauth.is_some()
    {
        return Ok(Some(ProfileCredentials {
            data: creds,
            source: CredentialSource::File(file_path),
        }));
    }

    // 4. If current, try default ~/.claude/.credentials.json
    if is_current
        && let Ok(default_claude) = profiles::default_claude_dir()
    {
        let default_file = default_claude.join(".credentials.json");
        if default_file.exists()
            && let Ok(content) = fs::read_to_string(&default_file)
            && let Ok(creds) = serde_json::from_str::<StoredCredentials>(&content)
            && creds.claude_ai_oauth.is_some()
        {
            return Ok(Some(ProfileCredentials {
                data: creds,
                source: CredentialSource::File(default_file),
            }));
        }
    }

    Ok(None)
}

pub fn is_token_expired(expires_at: Option<i64>) -> bool {
    let Some(exp) = expires_at else {
        return false;
    };
    let now_ms = Utc::now().timestamp_millis();
    now_ms + EXPIRY_BUFFER_MS >= exp
}

#[derive(Deserialize)]
struct TokenRefreshResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
    scope: Option<String>,
}

pub fn refresh_credentials(creds: &mut ProfileCredentials) -> Result<String> {
    let oauth = creds
        .data
        .claude_ai_oauth
        .as_mut()
        .ok_or_else(|| anyhow!("no OAuth data in credentials"))?;

    let refresh_tok = oauth
        .refresh_token
        .as_ref()
        .ok_or_else(|| anyhow!("no refresh token available for OAuth refresh"))?;

    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_tok,
        "client_id": OAUTH_CLIENT_ID
    });

    let req_body = serde_json::to_string(&body)?;

    let mut response = ureq::post(OAUTH_TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("User-Agent", "claude-user/0.2.0")
        .send(req_body.as_bytes())
        .context("failed to request refreshed token from Claude OAuth endpoint")?;

    let body_str = response
        .body_mut()
        .read_to_string()
        .context("failed to read token refresh response body")?;

    let resp_data: TokenRefreshResponse =
        serde_json::from_str(&body_str).context("failed to parse token refresh response JSON")?;

    let now_ms = Utc::now().timestamp_millis();
    oauth.access_token = resp_data.access_token.clone();
    oauth.expires_at = Some(now_ms + resp_data.expires_in * 1000);
    if let Some(r) = resp_data.refresh_token {
        oauth.refresh_token = Some(r);
    }
    if let Some(s) = resp_data.scope {
        oauth.scopes = Some(s.split_whitespace().map(String::from).collect());
    }

    // Save updated credentials back to storage
    save_credentials(creds)?;

    Ok(resp_data.access_token)
}

pub fn save_credentials(creds: &ProfileCredentials) -> Result<()> {
    let json = serde_json::to_string(&creds.data)?;
    match &creds.source {
        CredentialSource::Keychain(service) => {
            let account = get_keychain_account_name();
            write_keychain(service, &account, &json)?;
        }
        CredentialSource::File(path) => {
            fs::write(path, json)?;
        }
    }
    Ok(())
}
