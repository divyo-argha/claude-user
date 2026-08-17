use anyhow::{anyhow, bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProfileInfo {
    pub name: String,
    pub email: Option<String>,
    pub org_name: Option<String>,
}


pub fn profiles_root() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))?;
    Ok(home.join(".claude-profiles"))
}

pub fn shared_dir() -> Result<PathBuf> {
    Ok(profiles_root()?.join("shared"))
}

pub fn profile_dir(name: &str) -> Result<PathBuf> {
    Ok(profiles_root()?.join(name))
}

pub fn default_claude_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))?;
    Ok(home.join(".claude"))
}

pub fn default_claude_json() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))?;
    Ok(home.join(".claude.json"))
}

pub fn can_import() -> Result<bool> {
    let claude_dir = default_claude_dir()?;
    let claude_json = default_claude_json()?;
    Ok((claude_dir.exists() && !is_symlink(&claude_dir))
        || (claude_json.exists() && !is_symlink(&claude_json)))
}

fn is_symlink(path: &Path) -> bool {
    path.symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Checks whether ~/.claude / ~/.claude.json would block `activate_profile`, without
/// needing a specific profile directory to exist yet. Meant as a preflight before
/// creating a brand-new profile, so a blocked default doesn't leave an orphaned,
/// never-activated profile directory behind.
pub fn check_default_available() -> Result<()> {
    check_link_available(&default_claude_dir()?)?;
    check_link_available(&default_claude_json()?)?;
    Ok(())
}

fn check_link_available(link_path: &Path) -> Result<()> {
    if is_symlink(link_path) || !link_path.exists() {
        return Ok(());
    }
    bail!(
        "{} exists and isn't managed by cuser.\nRun `cuser import <name>` to bring it in as a profile first, or move it aside.",
        link_path.display()
    );
}

/// Points ~/.claude and ~/.claude.json at the given profile via symlinks, so that
/// `claude` run directly (without cuser) transparently uses whichever profile was
/// last activated. Refuses to touch either path if it's a real file/dir rather than
/// a cuser-managed symlink, so an existing default login is never silently clobbered.
pub fn activate_profile(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let dir = profile_dir(name)?;
    if !dir.exists() {
        return Ok(());
    }
    link_default(&default_claude_dir()?, &dir)?;
    link_default(&default_claude_json()?, &dir.join(".claude.json"))?;
    Ok(())
}

fn link_default(link_path: &Path, target: &Path) -> Result<()> {
    if is_symlink(link_path) {
        match fs::read_link(link_path) {
            Ok(current) if current == target => return Ok(()),
            _ => fs::remove_file(link_path)?,
        }
    } else {
        check_link_available(link_path)?;
    }
    create_symlink(target, link_path)
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_symlink(_target: &Path, _link: &Path) -> Result<()> {
    Ok(())
}

pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("profile name cannot be empty");
    }
    if name == "shared" {
        bail!("\"shared\" is reserved for the common config directory");
    }
    if name == "." || name == ".." {
        bail!("invalid profile name");
    }
    let safe = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !safe {
        bail!("profile names may only contain letters, numbers, '-' and '_'");
    }
    Ok(())
}

pub fn list_profiles() -> Result<Vec<String>> {
    let root = profiles_root()?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "shared" {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

pub fn get_profile_info(name: &str) -> Result<ProfileInfo> {
    let dir = profile_dir(name)?;
    let mut email = None;
    let mut org_name = None;

    let claude_json_path = dir.join(".claude.json");
    if claude_json_path.exists() {
        if let Ok(content) = fs::read_to_string(&claude_json_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(oauth) = val.get("oauthAccount") {
                    email = oauth.get("emailAddress").and_then(|v| v.as_str()).map(String::from);
                    org_name = oauth.get("organizationName").and_then(|v| v.as_str()).map(String::from);
                }
            }
        }
    }

    Ok(ProfileInfo {
        name: name.to_string(),
        email,
        org_name,
    })
}

pub fn profile_exists(name: &str) -> Result<bool> {
    Ok(profile_dir(name)?.exists())
}

pub fn remove_profile(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let dir = profile_dir(name)?;
    if !dir.exists() {
        bail!("profile \"{name}\" does not exist");
    }
    retarget_active(&dir, None)?;
    fs::remove_dir_all(&dir)?;
    Ok(())
}

pub fn rename_profile(old: &str, new: &str) -> Result<()> {
    validate_profile_name(old)?;
    validate_profile_name(new)?;
    let old_dir = profile_dir(old)?;
    if !old_dir.exists() {
        bail!("profile \"{old}\" does not exist");
    }
    let new_dir = profile_dir(new)?;
    if new_dir.exists() {
        bail!("profile \"{new}\" already exists");
    }
    retarget_active(&old_dir, Some(&new_dir))?;
    fs::rename(&old_dir, &new_dir)?;
    Ok(())
}

/// If ~/.claude (and ~/.claude.json) are cuser-managed symlinks pointing at
/// `old_dir`, repoints them at `new_dir` — or removes them if `new_dir` is `None`.
/// No-op if the default location isn't currently linked to `old_dir` at all.
fn retarget_active(old_dir: &Path, new_dir: Option<&Path>) -> Result<()> {
    retarget_link(&default_claude_dir()?, old_dir, new_dir)?;
    let old_json = old_dir.join(".claude.json");
    let new_json = new_dir.map(|d| d.join(".claude.json"));
    retarget_link(&default_claude_json()?, &old_json, new_json.as_deref())?;
    Ok(())
}

fn retarget_link(link_path: &Path, old_target: &Path, new_target: Option<&Path>) -> Result<()> {
    if !is_symlink(link_path) {
        return Ok(());
    }
    if fs::read_link(link_path)?.as_path() != old_target {
        return Ok(());
    }
    fs::remove_file(link_path)?;
    if let Some(new_target) = new_target {
        create_symlink(new_target, link_path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn harden_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_dir(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn harden_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_file(_path: &Path) -> Result<()> {
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    harden_dir(path)
}

fn copy_recursive(src: &Path, dst: &Path) -> Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if src.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
    }
    Ok(())
}

pub fn sync_profile(name: &str) -> Result<()> {
    let shared = shared_dir()?;
    if !shared.exists() {
        return Ok(());
    }
    let dest = profile_dir(name)?;
    create_private_dir(&dest)?;
    for entry in fs::read_dir(&shared)? {
        let entry = entry?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();
        if file_name_str == ".credentials.json" || file_name_str == ".claude.json" {
            continue;
        }
        copy_recursive(&src_path, &dest.join(file_name))?;
    }
    Ok(())
}

pub fn sync_all() -> Result<Vec<String>> {
    let names = list_profiles()?;
    for name in &names {
        sync_profile(name)?;
    }
    Ok(names)
}

pub fn create_profile(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    create_private_dir(&profiles_root()?)?;
    create_private_dir(&profile_dir(name)?)?;
    sync_profile(name)?;
    Ok(())
}

pub fn ensure_profile(name: &str) -> Result<bool> {
    validate_profile_name(name)?;
    if profile_exists(name)? {
        Ok(false)
    } else {
        create_profile(name)?;
        Ok(true)
    }
}

/// Moves the default ~/.claude and ~/.claude.json into a new profile,
/// ensuring onboarding state, oauth credentials, and settings are fully preserved.
pub fn import_default(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let claude_dir = default_claude_dir()?;
    let claude_json = default_claude_json()?;

    if !claude_dir.exists() && !claude_json.exists() {
        bail!("no existing ~/.claude directory or ~/.claude.json found to import");
    }
    if is_symlink(&claude_dir) || is_symlink(&claude_json) {
        bail!(
            "~/.claude is already managed by cuser (it's a symlink to an existing profile) — there's nothing new to import"
        );
    }
    let dest = profile_dir(name)?;
    if dest.exists() {
        bail!("profile \"{name}\" already exists");
    }

    create_private_dir(&profiles_root()?)?;
    let shared = shared_dir()?;
    create_private_dir(&shared)?;

    if claude_dir.exists() {
        if fs::rename(&claude_dir, &dest).is_err() {
            copy_recursive(&claude_dir, &dest)?;
            let _ = fs::remove_dir_all(&claude_dir);
        }
    } else {
        create_private_dir(&dest)?;
    }
    harden_dir(&dest)?;

    let dest_claude_json = dest.join(".claude.json");
    if claude_json.exists() {
        if let Ok(content) = fs::read_to_string(&claude_json) {
            if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("hasCompletedOnboarding".to_string(), serde_json::Value::Bool(true));
                }
                if let Ok(formatted) = serde_json::to_string_pretty(&val) {
                    let _ = fs::write(&dest_claude_json, formatted);
                } else {
                    let _ = fs::copy(&claude_json, &dest_claude_json);
                }
            } else {
                let _ = fs::copy(&claude_json, &dest_claude_json);
            }
        } else {
            let _ = fs::copy(&claude_json, &dest_claude_json);
        }
        let _ = fs::remove_file(&claude_json);
    } else if !dest_claude_json.exists() {
        let minimal = serde_json::json!({
            "hasCompletedOnboarding": true
        });
        let _ = fs::write(&dest_claude_json, serde_json::to_string_pretty(&minimal)?);
    }

    if dest_claude_json.exists() {
        let _ = harden_file(&dest_claude_json);
    }
    let dest_creds = dest.join(".credentials.json");
    if dest_creds.exists() {
        let _ = harden_file(&dest_creds);
    }

    let settings_src = dest.join("settings.json");
    if settings_src.exists() && !shared.join("settings.json").exists() {
        let _ = fs::copy(&settings_src, shared.join("settings.json"));
    }
    sync_profile(name)?;
    Ok(())
}

