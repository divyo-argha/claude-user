use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::oauth;
use crate::profiles;
use crate::usage;

pub const EXPORT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedProfile {
    pub name: String,
    #[serde(default)]
    pub is_disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_json: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupArchive {
    pub version: u32,
    pub exported_at: String,
    pub profiles: Vec<ExportedProfile>,
}

pub fn export_accounts(dest: Option<&str>, target_profile: Option<&str>) -> Result<()> {
    let all_names = profiles::list_profiles()?;
    let names_to_export: Vec<String> = if let Some(target) = target_profile {
        if !profiles::profile_exists(target)? {
            bail!("profile \"{target}\" does not exist");
        }
        vec![target.to_string()]
    } else {
        all_names
    };

    if names_to_export.is_empty() {
        bail!("no profiles available to export");
    }

    let mut exported_profiles = Vec::new();

    for name in &names_to_export {
        let info = profiles::get_profile_info(name)?;
        let dir = profiles::profile_dir(name)?;

        // Read .claude.json if present
        let claude_json_path = dir.join(".claude.json");
        let claude_json = if claude_json_path.exists() {
            fs::read_to_string(&claude_json_path)
                .ok()
                .and_then(|c| serde_json::from_str(&c).ok())
        } else {
            None
        };

        // Read credentials: check file or keychain
        let mut credentials_val = None;
        let creds_file = dir.join(".credentials.json");
        if creds_file.exists()
            && let Ok(c) = fs::read_to_string(&creds_file)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&c)
        {
            credentials_val = Some(val);
        }

        if credentials_val.is_none()
            && let Ok(Some(creds)) = oauth::load_profile_credentials(name)
        {
            credentials_val = serde_json::to_value(&creds.data).ok();
        }

        exported_profiles.push(ExportedProfile {
            name: name.clone(),
            is_disabled: info.is_disabled,
            email: info.email,
            org_name: info.org_name,
            claude_json,
            credentials: credentials_val,
        });
    }

    let archive = BackupArchive {
        version: EXPORT_FORMAT_VERSION,
        exported_at: Utc::now().to_rfc3339(),
        profiles: exported_profiles,
    };

    let json_text = serde_json::to_string_pretty(&archive)?;

    match dest {
        None | Some("-") => {
            println!("{json_text}");
        }
        Some(path_str) => {
            let path = Path::new(path_str);
            if path.is_dir() {
                bail!("export destination must be a file, not a directory: {path_str}");
            }
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, &json_text)?;
            #[cfg(unix)]
            {
                let _ = profiles::harden_file(path);
            }
            eprintln!(
                "Successfully exported {} profile(s) to {}",
                archive.profiles.len(),
                path.display()
            );
        }
    }

    Ok(())
}

pub fn import_accounts(path: &Path, force: bool) -> Result<()> {
    if !path.exists() {
        bail!("backup file does not exist: {}", path.display());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read backup file {}", path.display()))?;

    let archive: BackupArchive = serde_json::from_str(&content)
        .context("invalid backup format (expected valid JSON backup file)")?;

    if archive.version != EXPORT_FORMAT_VERSION {
        bail!(
            "unsupported backup version {} (expected {})",
            archive.version,
            EXPORT_FORMAT_VERSION
        );
    }

    if archive.profiles.is_empty() {
        println!("No profiles found in backup file.");
        return Ok(());
    }

    let mut imported_count = 0;
    let mut skipped_count = 0;

    for p in archive.profiles {
        let exists = profiles::profile_exists(&p.name)?;
        if exists && !force {
            println!(
                "Skipping existing profile \"{}\" (use --force to overwrite)",
                p.name
            );
            skipped_count += 1;
            continue;
        }

        profiles::ensure_profile(&p.name)?;
        let dir = profiles::profile_dir(&p.name)?;

        // Write .claude.json
        let mut cjson_val = p.claude_json.clone().unwrap_or_else(|| {
            serde_json::json!({
                "hasCompletedOnboarding": true
            })
        });
        if (p.email.is_some() || p.org_name.is_some())
            && cjson_val.get("oauthAccount").is_none()
            && let Some(obj) = cjson_val.as_object_mut()
        {
            obj.insert(
                "oauthAccount".to_string(),
                serde_json::json!({
                    "emailAddress": p.email,
                    "organizationName": p.org_name
                }),
            );
        }
        let cjson_path = dir.join(".claude.json");
        let formatted = serde_json::to_string_pretty(&cjson_val)?;
        fs::write(&cjson_path, formatted)?;
        let _ = profiles::harden_file(&cjson_path);

        // Write credentials
        if let Some(creds) = &p.credentials {
            let creds_json = serde_json::to_string_pretty(creds)?;
            let creds_path = dir.join(".credentials.json");
            fs::write(&creds_path, &creds_json)?;
            let _ = profiles::harden_file(&creds_path);

            // On macOS: sync to Keychain
            #[cfg(target_os = "macos")]
            {
                let service = oauth::get_keychain_service_name(&dir);
                let account = oauth::get_keychain_account_name();
                let _ = oauth::write_keychain(&service, &account, &creds_json);
            }
        }

        // Set disabled state
        profiles::set_profile_disabled(&p.name, p.is_disabled)?;

        let detail = match (p.email, p.org_name) {
            (Some(e), Some(o)) => format!(" ({e} • {o})"),
            (Some(e), None) => format!(" ({e})"),
            _ => String::new(),
        };
        println!("Imported profile \"{}\"{detail}", p.name);
        imported_count += 1;
    }

    let _ = usage::invalidate_cache();

    println!("Import complete: {imported_count} imported, {skipped_count} skipped.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_archive_serialization() {
        let profile = ExportedProfile {
            name: "test-profile".to_string(),
            is_disabled: false,
            email: Some("test@example.com".to_string()),
            org_name: Some("Test Org".to_string()),
            claude_json: Some(serde_json::json!({"hasCompletedOnboarding": true})),
            credentials: Some(serde_json::json!({"apiKey": "sk-ant-api03-test"})),
        };

        let archive = BackupArchive {
            version: EXPORT_FORMAT_VERSION,
            exported_at: "2026-10-04T00:00:00Z".to_string(),
            profiles: vec![profile],
        };

        let json = serde_json::to_string(&archive).unwrap();
        let parsed: BackupArchive = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.profiles.len(), 1);
        assert_eq!(parsed.profiles[0].name, "test-profile");
        assert_eq!(
            parsed.profiles[0].email.as_deref(),
            Some("test@example.com")
        );
    }

    #[test]
    fn test_import_accounts_flow() {
        let test_profile_name = "test_import_unique_xyz123";
        let _ = profiles::remove_profile(test_profile_name);

        let profile = ExportedProfile {
            name: test_profile_name.to_string(),
            is_disabled: false,
            email: Some("import_test@example.com".to_string()),
            org_name: Some("Import Org".to_string()),
            claude_json: Some(serde_json::json!({"hasCompletedOnboarding": true})),
            credentials: Some(serde_json::json!({"apiKey": "sk-ant-api03-testkey"})),
        };

        let archive = BackupArchive {
            version: EXPORT_FORMAT_VERSION,
            exported_at: "2026-10-04T00:00:00Z".to_string(),
            profiles: vec![profile],
        };

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_backup_cuser.json");
        fs::write(&file_path, serde_json::to_string_pretty(&archive).unwrap()).unwrap();

        let result = import_accounts(&file_path, true);
        assert!(result.is_ok());
        assert!(profiles::profile_exists(test_profile_name).unwrap());

        let info = profiles::get_profile_info(test_profile_name).unwrap();
        assert_eq!(info.email.as_deref(), Some("import_test@example.com"));

        // Clean up
        let _ = profiles::remove_profile(test_profile_name);
        let _ = fs::remove_file(file_path);
    }
}
