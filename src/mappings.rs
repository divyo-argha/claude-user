use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::profiles;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapListEntry {
    pub path: String,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapOutput {
    pub cwd: String,
    pub active_mapping: Option<MapListEntry>,
    pub mappings: Vec<MapListEntry>,
}

pub fn mappings_file() -> Result<PathBuf> {
    Ok(profiles::profiles_root()?.join("mappings.json"))
}

pub fn load_mappings() -> Result<BTreeMap<PathBuf, String>> {
    let file = mappings_file()?;
    if !file.exists() {
        return Ok(BTreeMap::new());
    }
    let content = fs::read_to_string(&file)
        .with_context(|| format!("failed to read mappings file: {}", file.display()))?;
    if content.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let map: BTreeMap<String, String> = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse mappings file: {}", file.display()))?;

    let mut result = BTreeMap::new();
    for (k, v) in map {
        result.insert(PathBuf::from(k), v);
    }
    Ok(result)
}

pub fn save_mappings(mappings: &BTreeMap<PathBuf, String>) -> Result<()> {
    let root = profiles::profiles_root()?;
    if !root.exists() {
        fs::create_dir_all(&root)?;
    }
    let file = mappings_file()?;
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in mappings {
        map.insert(k.to_string_lossy().into_owned(), v.clone());
    }
    let json = serde_json::to_string_pretty(&map)?;
    fs::write(&file, json)
        .with_context(|| format!("failed to write mappings file: {}", file.display()))?;
    Ok(())
}

pub fn resolve_path(input: Option<&str>) -> Result<PathBuf> {
    let path = match input {
        Some(p) => {
            let p_trim = p.trim();
            if p_trim.starts_with("~/") || p_trim == "~" {
                let home = dirs::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))?;
                if p_trim == "~" {
                    home
                } else {
                    home.join(&p_trim[2..])
                }
            } else {
                PathBuf::from(p_trim)
            }
        }
        None => std::env::current_dir()?,
    };

    if !path.exists() {
        bail!("directory does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("path is not a directory: {}", path.display());
    }

    path.canonicalize()
        .with_context(|| format!("failed to canonicalize path: {}", path.display()))
}

pub fn add_mapping(profile: &str, dir_input: Option<&str>) -> Result<PathBuf> {
    profiles::validate_profile_name(profile)?;
    if !profiles::profile_exists(profile)? {
        bail!("profile \"{profile}\" does not exist");
    }
    let path = resolve_path(dir_input)?;
    let mut mappings = load_mappings()?;
    mappings.insert(path.clone(), profile.to_string());
    save_mappings(&mappings)?;
    Ok(path)
}

pub fn remove_mapping(dir_input: Option<&str>) -> Result<(PathBuf, Option<String>)> {
    let path = resolve_path(dir_input)?;
    let mut mappings = load_mappings()?;
    let prev = mappings.remove(&path);
    if prev.is_some() {
        save_mappings(&mappings)?;
    }
    Ok((path, prev))
}

pub fn resolve_mapping(dir: &Path) -> Result<Option<(String, PathBuf)>> {
    let mappings = load_mappings()?;
    if mappings.is_empty() {
        return Ok(None);
    }
    let canon = if dir.is_absolute() {
        dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(dir)
    };

    for ancestor in canon.ancestors() {
        if let Some(profile) = mappings.get(ancestor) {
            return Ok(Some((profile.clone(), ancestor.to_path_buf())));
        }
    }
    Ok(None)
}

pub fn list_mappings() -> Result<Vec<(PathBuf, String)>> {
    let mappings = load_mappings()?;
    Ok(mappings.into_iter().collect())
}

pub fn mappings_for_profile(profile: &str) -> Result<Vec<PathBuf>> {
    let mappings = load_mappings()?;
    Ok(mappings
        .into_iter()
        .filter(|(_, p)| p == profile)
        .map(|(path, _)| path)
        .collect())
}

pub fn remove_profile_mappings(profile: &str) -> Result<usize> {
    let mut mappings = load_mappings()?;
    let initial_len = mappings.len();
    mappings.retain(|_, p| p != profile);
    let removed = initial_len - mappings.len();
    if removed > 0 {
        save_mappings(&mappings)?;
    }
    Ok(removed)
}

pub fn rename_profile_mappings(old: &str, new: &str) -> Result<usize> {
    let mut mappings = load_mappings()?;
    let mut renamed = 0;
    for p in mappings.values_mut() {
        if p == old {
            *p = new.to_string();
            renamed += 1;
        }
    }
    if renamed > 0 {
        save_mappings(&mappings)?;
    }
    Ok(renamed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mappings_serialization() {
        let mut map = BTreeMap::new();
        map.insert(PathBuf::from("/tmp/work"), "work".to_string());
        map.insert(PathBuf::from("/tmp/personal"), "personal".to_string());

        let mut string_map = BTreeMap::new();
        for (k, v) in &map {
            string_map.insert(k.to_string_lossy().into_owned(), v.clone());
        }
        let serialized = serde_json::to_string(&string_map).unwrap();
        let parsed: BTreeMap<String, String> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed.get("/tmp/work").unwrap(), "work");
        assert_eq!(parsed.get("/tmp/personal").unwrap(), "personal");
    }

    #[test]
    fn test_ancestor_resolution_logic() {
        let mut map = BTreeMap::new();
        map.insert(PathBuf::from("/Users/test/workspace/client"), "client".to_string());
        map.insert(PathBuf::from("/Users/test/personal"), "personal".to_string());

        let target = Path::new("/Users/test/workspace/client/src/components");
        let mut resolved = None;
        for ancestor in target.ancestors() {
            if let Some(profile) = map.get(ancestor) {
                resolved = Some((profile.clone(), ancestor.to_path_buf()));
                break;
            }
        }

        assert_eq!(
            resolved,
            Some(("client".to_string(), PathBuf::from("/Users/test/workspace/client")))
        );

        let unmapped = Path::new("/Users/test/other/path");
        let mut unmapped_resolved = None;
        for ancestor in unmapped.ancestors() {
            if let Some(profile) = map.get(ancestor) {
                unmapped_resolved = Some((profile.clone(), ancestor.to_path_buf()));
                break;
            }
        }
        assert!(unmapped_resolved.is_none());
    }
}
