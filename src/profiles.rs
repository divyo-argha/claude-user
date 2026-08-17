use anyhow::{anyhow, bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

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

pub fn can_import() -> Result<bool> {
    Ok(default_claude_dir()?.exists())
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

pub fn profile_exists(name: &str) -> Result<bool> {
    Ok(profile_dir(name)?.exists())
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
    fs::create_dir_all(&dest)?;
    for entry in fs::read_dir(&shared)? {
        let entry = entry?;
        copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
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
    fs::create_dir_all(profile_dir(name)?)?;
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

/// Moves the default ~/.claude (whatever account is currently logged into it)
/// into a new profile, splitting settings.json out into shared/. Works any time
/// ~/.claude exists, not just on first run, so re-logging into the default
/// location is always a valid way to bring in another account.
pub fn import_default(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let claude_dir = default_claude_dir()?;
    if !claude_dir.exists() {
        bail!("no existing ~/.claude directory found to import");
    }
    let dest = profile_dir(name)?;
    if dest.exists() {
        bail!("profile \"{name}\" already exists");
    }

    let shared = shared_dir()?;
    fs::create_dir_all(&shared)?;
    fs::rename(&claude_dir, &dest)?;

    let settings_src = dest.join("settings.json");
    if settings_src.exists() {
        fs::copy(&settings_src, shared.join("settings.json"))?;
    }
    sync_profile(name)?;
    Ok(())
}
