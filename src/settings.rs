use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::profiles;

pub fn settings_file() -> Result<PathBuf> {
    Ok(profiles::profiles_root()?.join("settings.json"))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoSwitchConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoswitch: Option<AutoSwitchConfig>,
}

impl ToolSettings {
    pub fn load() -> Result<Self> {
        let file = settings_file()?;
        if !file.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&file)?;
        if content.trim().is_empty() {
            return Ok(Self::default());
        }
        let s: Self = serde_json::from_str(&content).unwrap_or_default();
        Ok(s)
    }

    pub fn save(&self) -> Result<()> {
        let root = profiles::profiles_root()?;
        if !root.exists() {
            fs::create_dir_all(&root)?;
        }
        let file = settings_file()?;
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&file, json)?;
        Ok(())
    }

    pub fn effective_threshold(&self) -> f64 {
        self.autoswitch
            .as_ref()
            .and_then(|a| a.threshold)
            .unwrap_or(90.0)
    }

    pub fn effective_strategy(&self) -> &str {
        self.autoswitch
            .as_ref()
            .and_then(|a| a.strategy.as_deref())
            .unwrap_or("best")
    }

    pub fn effective_model(&self) -> Option<String> {
        self.autoswitch.as_ref().and_then(|a| a.model.clone())
    }

    pub fn effective_interval(&self) -> u64 {
        self.autoswitch
            .as_ref()
            .and_then(|a| a.interval)
            .unwrap_or(60)
    }
}

pub fn get_setting(key: &str) -> Result<Option<String>> {
    let s = ToolSettings::load()?;
    match key {
        "autoswitch.threshold" => Ok(s
            .autoswitch
            .as_ref()
            .and_then(|a| a.threshold)
            .map(|v| v.to_string())),
        "autoswitch.strategy" => Ok(s.autoswitch.as_ref().and_then(|a| a.strategy.clone())),
        "autoswitch.model" => Ok(s.autoswitch.as_ref().and_then(|a| a.model.clone())),
        "autoswitch.interval" => Ok(s
            .autoswitch
            .as_ref()
            .and_then(|a| a.interval)
            .map(|v| v.to_string())),
        other => bail!(
            "unknown setting key: '{other}'. Supported keys:\n  autoswitch.threshold\n  autoswitch.strategy\n  autoswitch.model\n  autoswitch.interval"
        ),
    }
}

pub fn set_setting(key: &str, val: &str) -> Result<()> {
    let mut s = ToolSettings::load()?;
    let auto = s.autoswitch.get_or_insert_with(AutoSwitchConfig::default);

    match key {
        "autoswitch.threshold" => {
            let num = val.parse::<f64>()?;
            if !(1.0..=100.0).contains(&num) {
                bail!("autoswitch.threshold must be between 1.0 and 100.0");
            }
            auto.threshold = Some(num);
        }
        "autoswitch.strategy" => {
            let lower = val.to_lowercase();
            if lower != "best" && lower != "next-available" && lower != "next" && lower != "consume-first" {
                bail!("autoswitch.strategy must be 'best', 'next-available', or 'consume-first'");
            }
            auto.strategy = Some(lower);
        }
        "autoswitch.model" => {
            if val.trim().is_empty() {
                auto.model = None;
            } else {
                auto.model = Some(val.to_string());
            }
        }
        "autoswitch.interval" => {
            let num = val.parse::<u64>()?;
            if num < 5 {
                bail!("autoswitch.interval must be at least 5 seconds");
            }
            auto.interval = Some(num);
        }
        other => bail!("unknown setting key: '{other}'"),
    }

    s.save()?;
    println!("Set {key} = {val}");
    Ok(())
}

pub fn unset_setting(key: &str) -> Result<()> {
    let mut s = ToolSettings::load()?;
    if let Some(auto) = s.autoswitch.as_mut() {
        match key {
            "autoswitch.threshold" => auto.threshold = None,
            "autoswitch.strategy" => auto.strategy = None,
            "autoswitch.model" => auto.model = None,
            "autoswitch.interval" => auto.interval = None,
            other => bail!("unknown setting key: '{other}'"),
        }
    }
    s.save()?;
    println!("Unset {key} (restored default)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effective_defaults() {
        let s = ToolSettings::default();
        assert_eq!(s.effective_threshold(), 90.0);
        assert_eq!(s.effective_strategy(), "best");
        assert_eq!(s.effective_interval(), 60);
        assert!(s.effective_model().is_none());
    }
}
