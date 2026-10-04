use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::profiles;

pub fn aliases_file() -> Result<PathBuf> {
    Ok(profiles::profiles_root()?.join("aliases.json"))
}

pub fn load_aliases() -> Result<BTreeMap<String, String>> {
    let file = aliases_file()?;
    if !file.exists() {
        return Ok(BTreeMap::new());
    }
    let content = fs::read_to_string(&file)?;
    if content.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let map: BTreeMap<String, String> = serde_json::from_str(&content).unwrap_or_default();
    Ok(map)
}

pub fn save_aliases(aliases: &BTreeMap<String, String>) -> Result<()> {
    let root = profiles::profiles_root()?;
    if !root.exists() {
        fs::create_dir_all(&root)?;
    }
    let file = aliases_file()?;
    let json = serde_json::to_string_pretty(aliases)?;
    fs::write(&file, json)?;
    Ok(())
}

pub fn set_alias_quiet(profile_arg: &str, alias: &str) -> Result<()> {
    let target_profile = resolve_profile_name(profile_arg)?;
    if !profiles::profile_exists(&target_profile)? {
        bail!("profile \"{target_profile}\" does not exist");
    }
    profiles::validate_profile_name(alias)?;
    if profiles::profile_exists(alias)? && alias != target_profile {
        bail!("cannot set alias \"{alias}\": a profile with this name already exists");
    }
    let mut aliases = load_aliases()?;
    aliases.insert(alias.to_string(), target_profile);
    save_aliases(&aliases)?;
    Ok(())
}

pub fn set_alias(profile_arg: &str, alias: &str) -> Result<()> {
    let target_profile = resolve_profile_name(profile_arg)?;
    set_alias_quiet(&target_profile, alias)?;
    println!("Aliased \"{alias}\" -> \"{target_profile}\".");
    Ok(())
}

pub fn remove_alias(alias: &str) -> Result<Option<String>> {
    let mut aliases = load_aliases()?;
    let removed = aliases.remove(alias);
    if removed.is_some() {
        save_aliases(&aliases)?;
    }
    Ok(removed)
}

pub fn remove_aliases_for_profile(profile: &str) -> Result<()> {
    let mut aliases = load_aliases()?;
    let before_len = aliases.len();
    aliases.retain(|_, p| p != profile);
    if aliases.len() != before_len {
        save_aliases(&aliases)?;
    }
    Ok(())
}

pub fn rename_profile_aliases(old: &str, new: &str) -> Result<()> {
    let mut aliases = load_aliases()?;
    let mut modified = false;
    for p in aliases.values_mut() {
        if *p == old {
            *p = new.to_string();
            modified = true;
        }
    }
    if modified {
        save_aliases(&aliases)?;
    }
    Ok(())
}

/// Resolves an alias to its underlying profile name. If the input name is not an alias,
/// returns the input unchanged.
pub fn resolve_profile_name(name_or_alias: &str) -> Result<String> {
    let aliases = load_aliases()?;
    if let Some(target) = aliases.get(name_or_alias) {
        return Ok(target.clone());
    }
    Ok(name_or_alias.to_string())
}

pub fn aliases_for_profile(profile: &str) -> Result<Vec<String>> {
    let aliases = load_aliases()?;
    let mut matched = Vec::new();
    for (alias, target) in aliases {
        if target == profile {
            matched.push(alias);
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aliases_map() {
        let mut map = BTreeMap::new();
        map.insert("dev".to_string(), "work".to_string());
        map.insert("me".to_string(), "personal".to_string());

        assert_eq!(map.get("dev").unwrap(), "work");
        assert_eq!(map.get("me").unwrap(), "personal");
        assert!(!map.contains_key("other"));
    }
}
