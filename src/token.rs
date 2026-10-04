use anyhow::{bail, Result};
use std::fs;
use std::io::{self, Read};

use crate::oauth;
use crate::profiles;
use crate::usage;

pub fn add_token(
    raw_token_arg: &str,
    profile_name: &str,
    email_override: Option<&str>,
    org_override: Option<&str>,
) -> Result<()> {
    profiles::validate_profile_name(profile_name)?;

    let token = if raw_token_arg == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf.trim().to_string()
    } else {
        raw_token_arg.trim().to_string()
    };

    if token.is_empty() {
        bail!("token cannot be empty");
    }

    let is_oauth = token.starts_with("sk-ant-oat01-") || token.starts_with("sk-ant-oat");
    let is_api_key = token.starts_with("sk-ant-api");

    if !is_oauth && !is_api_key {
        bail!(
            "invalid token format: must start with 'sk-ant-oat01-' (OAuth setup token) or 'sk-ant-api' (Anthropic API key)"
        );
    }

    let email = email_override.map(String::from).unwrap_or_else(|| {
        if is_oauth {
            format!("setup-token-{profile_name}@token.local")
        } else {
            format!("api-key-{profile_name}@token.local")
        }
    });

    let org_name = org_override.map(String::from).or_else(|| {
        if is_oauth {
            Some("OAuth Setup Token".to_string())
        } else {
            Some("Anthropic API Key".to_string())
        }
    });

    profiles::ensure_profile(profile_name)?;
    let dir = profiles::profile_dir(profile_name)?;

    // 1. Build .claude.json
    let cjson_path = dir.join(".claude.json");
    let mut claude_config = if cjson_path.exists()
        && let Ok(content) = fs::read_to_string(&cjson_path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
    {
        val
    } else {
        serde_json::json!({
            "hasCompletedOnboarding": true,
        })
    };

    if let Some(obj) = claude_config.as_object_mut() {
        obj.insert(
            "hasCompletedOnboarding".to_string(),
            serde_json::Value::Bool(true),
        );
        let oauth_acc = serde_json::json!({
            "emailAddress": email,
            "organizationName": org_name,
        });
        obj.insert("oauthAccount".to_string(), oauth_acc);
        if is_api_key {
            obj.insert(
                "primaryApiKey".to_string(),
                serde_json::Value::String(token.clone()),
            );
        }
    }

    let cjson_str = serde_json::to_string_pretty(&claude_config)?;
    fs::write(&cjson_path, cjson_str)?;
    let _ = profiles::harden_file(&cjson_path);

    // 2. Build .credentials.json
    let creds_val = if is_oauth {
        serde_json::json!({
            "claudeAiOauth": {
                "accessToken": token,
                "refreshToken": null,
                "expiresAt": null,
                "scopes": ["user:inference", "user:sessions"]
            }
        })
    } else {
        serde_json::json!({
            "apiKey": token
        })
    };

    let creds_json = serde_json::to_string_pretty(&creds_val)?;
    let creds_path = dir.join(".credentials.json");
    fs::write(&creds_path, &creds_json)?;
    let _ = profiles::harden_file(&creds_path);

    // 3. On macOS: write to Keychain
    #[cfg(target_os = "macos")]
    {
        let service = oauth::get_keychain_service_name(&dir);
        let account = oauth::get_keychain_account_name();
        let _ = oauth::write_keychain(&service, &account, &creds_json);

        if profiles::current_profile()?.as_deref() == Some(profile_name) {
            let _ = oauth::write_keychain("Claude Code-credentials", &account, &creds_json);
        }
    }

    let _ = usage::invalidate_cache();

    let kind_label = if is_oauth {
        "OAuth setup token"
    } else {
        "managed API key"
    };
    println!("Successfully registered profile \"{profile_name}\" ({email}) with {kind_label}.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_validation() {
        assert!(add_token("sk-ant-oat01-1234567890", "valid_name_test", None, None).is_ok());
        let _ = profiles::remove_profile("valid_name_test");

        assert!(add_token("invalid_prefix", "bad_token_test", None, None).is_err());
    }
}
