use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::oauth;
use crate::profiles;

const USAGE_API_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CACHE_TTL_SECS: i64 = 300; // 5 minutes

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowUsage {
    pub pct: f64,
    pub resets_at: Option<String>,
    pub countdown: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendUsage {
    pub used: f64,
    pub limit: f64,
    pub pct: f64,
    pub currency: String,
    pub resets_at: Option<String>,
    pub countdown: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQuota {
    pub name: String,
    pub pct: f64,
    pub resets_at: Option<String>,
    pub countdown: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageStatus {
    Ok,
    RateLimited,
    TokenExpired,
    NoUsageAccess,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountUsage {
    pub profile: String,
    pub status: UsageStatus,
    pub five_hour: Option<WindowUsage>,
    pub seven_day: Option<WindowUsage>,
    pub spend: Option<SpendUsage>,
    pub models: Vec<ModelQuota>,
    pub fetched_at: i64,
    pub error_message: Option<String>,
}

pub fn usage_cache_file() -> Result<PathBuf> {
    Ok(profiles::profiles_root()?.join("usage_cache.json"))
}

pub fn load_usage_cache() -> Result<HashMap<String, AccountUsage>> {
    let file = usage_cache_file()?;
    if !file.exists() {
        return Ok(HashMap::new());
    }
    let content = fs::read_to_string(&file)?;
    if content.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, AccountUsage> = serde_json::from_str(&content).unwrap_or_default();
    Ok(map)
}

pub fn save_usage_cache(cache: &HashMap<String, AccountUsage>) -> Result<()> {
    let root = profiles::profiles_root()?;
    if !root.exists() {
        fs::create_dir_all(&root)?;
    }
    let file = usage_cache_file()?;
    let json = serde_json::to_string_pretty(cache)?;
    fs::write(file, json)?;
    Ok(())
}

pub fn invalidate_cache() -> Result<()> {
    let file = usage_cache_file()?;
    if file.exists() {
        let _ = fs::remove_file(file);
    }
    Ok(())
}

pub fn format_reset_countdown(iso_str: &str) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(iso_str).ok()?;
    let now = Utc::now();
    let diff = dt.signed_duration_since(now);
    if diff.num_seconds() <= 0 {
        return Some("resetting now".to_string());
    }
    let total_mins = diff.num_minutes();
    let days = total_mins / (24 * 60);
    let hours = (total_mins % (24 * 60)) / 60;
    let mins = total_mins % 60;

    if days > 0 {
        Some(format!("{days}d {hours}h"))
    } else if hours > 0 {
        Some(format!("{hours}h {mins}m"))
    } else {
        Some(format!("{mins}m"))
    }
}

pub fn render_progress_bar(pct: f64, width: usize) -> String {
    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * (width as f64)).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}] {:>3.0}%", "█".repeat(filled), "░".repeat(empty), clamped)
}

pub fn get_profile_usage(profile: &str, force_refresh: bool) -> Result<AccountUsage> {
    let mut cache = load_usage_cache().unwrap_or_default();
    let now = Utc::now().timestamp();

    if !force_refresh
        && let Some(cached) = cache.get(profile)
        && now - cached.fetched_at < CACHE_TTL_SECS
    {
        let mut refreshed = cached.clone();
        // Refresh countdown strings from cached resets_at
        if let Some(h5) = &mut refreshed.five_hour
            && let Some(resets) = &h5.resets_at
        {
            h5.countdown = format_reset_countdown(resets);
        }
        if let Some(d7) = &mut refreshed.seven_day
            && let Some(resets) = &d7.resets_at
        {
            d7.countdown = format_reset_countdown(resets);
        }
        return Ok(refreshed);
    }

    let usage = fetch_profile_usage_live(profile)?;
    cache.insert(profile.to_string(), usage.clone());
    let _ = save_usage_cache(&cache);
    Ok(usage)
}

fn fetch_profile_usage_live(profile: &str) -> Result<AccountUsage> {
    let now = Utc::now().timestamp();
    let creds_opt = oauth::load_profile_credentials(profile)?;
    let mut creds = match creds_opt {
        Some(c) => c,
        None => {
            return Ok(AccountUsage {
                profile: profile.to_string(),
                status: UsageStatus::Unavailable,
                five_hour: None,
                seven_day: None,
                spend: None,
                models: Vec::new(),
                fetched_at: now,
                error_message: Some("no credentials found for profile".to_string()),
            });
        }
    };

    let mut access_token = match &creds.data.claude_ai_oauth {
        Some(o) => o.access_token.clone(),
        None => {
            return Ok(AccountUsage {
                profile: profile.to_string(),
                status: UsageStatus::Unavailable,
                five_hour: None,
                seven_day: None,
                spend: None,
                models: Vec::new(),
                fetched_at: now,
                error_message: Some("no OAuth credentials configured".to_string()),
            });
        }
    };

    // If expired, refresh upfront
    if oauth::is_token_expired(creds.data.claude_ai_oauth.as_ref().and_then(|o| o.expires_at))
        && let Ok(new_token) = oauth::refresh_credentials(&mut creds)
    {
        access_token = new_token;
    }

    match call_usage_api(&access_token) {
        Ok(data) => {
            let mut usage = parse_usage_response(profile, &data);
            usage.fetched_at = now;
            Ok(usage)
        }
        Err(e) => {
            // Check if 401 unauthorized -> try refresh
            let err_str = e.to_string();
            if err_str.contains("401") || err_str.contains("authentication_error") {
                if let Ok(new_token) = oauth::refresh_credentials(&mut creds)
                    && let Ok(data) = call_usage_api(&new_token)
                {
                    let mut usage = parse_usage_response(profile, &data);
                    usage.fetched_at = now;
                    return Ok(usage);
                }
                return Ok(AccountUsage {
                    profile: profile.to_string(),
                    status: UsageStatus::TokenExpired,
                    five_hour: None,
                    seven_day: None,
                    spend: None,
                    models: Vec::new(),
                    fetched_at: now,
                    error_message: Some("OAuth token expired and could not be refreshed".to_string()),
                });
            }

            if err_str.contains("429") {
                return Ok(AccountUsage {
                    profile: profile.to_string(),
                    status: UsageStatus::RateLimited,
                    five_hour: None,
                    seven_day: None,
                    spend: None,
                    models: Vec::new(),
                    fetched_at: now,
                    error_message: Some("Rate limited by Anthropic usage API (429)".to_string()),
                });
            }

            if err_str.contains("oauth_not_allowed_for_organization")
                || err_str.contains("permission_error")
            {
                return Ok(AccountUsage {
                    profile: profile.to_string(),
                    status: UsageStatus::NoUsageAccess,
                    five_hour: None,
                    seven_day: None,
                    spend: None,
                    models: Vec::new(),
                    fetched_at: now,
                    error_message: Some("OAuth usage API not supported for this account tier".to_string()),
                });
            }

            Ok(AccountUsage {
                profile: profile.to_string(),
                status: UsageStatus::Unavailable,
                five_hour: None,
                seven_day: None,
                spend: None,
                models: Vec::new(),
                fetched_at: now,
                error_message: Some(err_str),
            })
        }
    }
}

fn call_usage_api(access_token: &str) -> Result<serde_json::Value> {
    let mut response = ureq::get(USAGE_API_URL)
        .header("Authorization", &format!("Bearer {access_token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-user/0.2.0")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => anyhow!("usage API returned HTTP {code}"),
            other => anyhow::Error::new(other),
        })?;

    let body = response
        .body_mut()
        .read_to_string()
        .context("failed to read usage API response body")?;

    let val: serde_json::Value =
        serde_json::from_str(&body).context("failed to parse usage API JSON response")?;

    if let Some(err) = val.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown API error");
        let code = err
            .get("details")
            .and_then(|d| d.get("error_code"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        return Err(anyhow!("{msg} {code}"));
    }

    Ok(val)
}

fn parse_usage_response(profile: &str, data: &serde_json::Value) -> AccountUsage {
    let five_hour = data.get("five_hour").and_then(|h5| {
        let pct = h5.get("utilization")?.as_f64()?;
        let resets_at = h5.get("resets_at").and_then(|r| r.as_str()).map(String::from);
        let countdown = resets_at.as_deref().and_then(format_reset_countdown);
        Some(WindowUsage {
            pct,
            resets_at,
            countdown,
        })
    });

    let seven_day = data.get("seven_day").and_then(|d7| {
        let pct = d7.get("utilization")?.as_f64()?;
        let resets_at = d7.get("resets_at").and_then(|r| r.as_str()).map(String::from);
        let countdown = resets_at.as_deref().and_then(format_reset_countdown);
        Some(WindowUsage {
            pct,
            resets_at,
            countdown,
        })
    });

    let spend = data.get("extra_usage").and_then(|eu| {
        if !eu.get("is_enabled").and_then(|b| b.as_bool()).unwrap_or(false) {
            return None;
        }
        let used_cents = eu.get("used_credits")?.as_f64()?;
        let limit_cents = eu.get("monthly_limit")?.as_f64()?;
        let pct = eu.get("utilization")?.as_f64()?;
        let currency = eu
            .get("currency")
            .and_then(|c| c.as_str())
            .unwrap_or("USD")
            .to_string();
        let resets_at = eu.get("resets_at").and_then(|r| r.as_str()).map(String::from);
        let countdown = resets_at.as_deref().and_then(format_reset_countdown);

        Some(SpendUsage {
            used: used_cents / 100.0,
            limit: limit_cents / 100.0,
            pct,
            currency,
            resets_at,
            countdown,
        })
    });

    let mut models = Vec::new();
    if let Some(limits) = data.get("limits").and_then(|l| l.as_array()) {
        for lim in limits {
            let name = lim
                .get("scope")
                .and_then(|s| s.get("model"))
                .and_then(|m| m.get("display_name"))
                .and_then(|n| n.as_str());
            let pct = lim.get("percent").and_then(|p| p.as_f64());
            if let (Some(name), Some(pct)) = (name, pct) {
                let resets_at = lim.get("resets_at").and_then(|r| r.as_str()).map(String::from);
                let countdown = resets_at.as_deref().and_then(format_reset_countdown);
                models.push(ModelQuota {
                    name: name.to_string(),
                    pct,
                    resets_at,
                    countdown,
                });
            }
        }
    }

    AccountUsage {
        profile: profile.to_string(),
        status: UsageStatus::Ok,
        five_hour,
        seven_day,
        spend,
        models,
        fetched_at: Utc::now().timestamp(),
        error_message: None,
    }
}

pub fn import_usage_data(json_str: &str, _hold_secs: u64) -> Result<usize> {
    let mut cache = load_usage_cache().unwrap_or_default();
    let now = Utc::now().timestamp();
    let mut imported = 0;

    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
        let items: Vec<serde_json::Value> = if let Some(arr) = val.get("profiles").and_then(|p| p.as_array()) {
            arr.clone()
        } else if let Some(arr) = val.as_array() {
            arr.clone()
        } else {
            vec![val.clone()]
        };

        for item in items {
            let mut usage_opt: Option<AccountUsage> = None;
            let mut target_name = None;

            if let Some(u_val) = item.get("usage") {
                if let Ok(u) = serde_json::from_value::<AccountUsage>(u_val.clone()) {
                    usage_opt = Some(u);
                }
            } else if let Ok(u) = serde_json::from_value::<AccountUsage>(item.clone()) {
                usage_opt = Some(u);
            }

            if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                target_name = Some(name.to_string());
            }

            if let Some(mut u) = usage_opt {
                let name = target_name.unwrap_or_else(|| u.profile.clone());
                u.profile = name.clone();
                u.fetched_at = now;
                cache.insert(name, u);
                imported += 1;
            }
        }
    }

    if imported > 0 {
        save_usage_cache(&cache)?;
    }

    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_progress_bar() {
        let bar_0 = render_progress_bar(0.0, 10);
        assert_eq!(bar_0, "[░░░░░░░░░░]   0%");

        let bar_50 = render_progress_bar(50.0, 10);
        assert_eq!(bar_50, "[█████░░░░░]  50%");

        let bar_100 = render_progress_bar(100.0, 10);
        assert_eq!(bar_100, "[██████████] 100%");

        let bar_clamp = render_progress_bar(150.0, 10);
        assert_eq!(bar_clamp, "[██████████] 100%");
    }

    #[test]
    fn test_parse_usage_response() {
        let raw = serde_json::json!({
            "five_hour": {
                "utilization": 24.5,
                "resets_at": "2030-01-01T00:00:00Z"
            },
            "seven_day": {
                "utilization": 78.2,
                "resets_at": "2030-01-07T00:00:00Z"
            },
            "extra_usage": {
                "is_enabled": true,
                "used_credits": 350,
                "monthly_limit": 5000,
                "utilization": 7.0,
                "currency": "USD"
            },
            "limits": [
                {
                    "scope": {
                        "model": {
                            "display_name": "Sonnet"
                        }
                    },
                    "percent": 45.0
                }
            ]
        });

        let parsed = parse_usage_response("work", &raw);
        assert_eq!(parsed.profile, "work");
        assert_eq!(parsed.status, UsageStatus::Ok);
        assert_eq!(parsed.five_hour.as_ref().unwrap().pct, 24.5);
        assert_eq!(parsed.seven_day.as_ref().unwrap().pct, 78.2);
        assert_eq!(parsed.spend.as_ref().unwrap().used, 3.5);
        assert_eq!(parsed.spend.as_ref().unwrap().limit, 50.0);
        assert_eq!(parsed.models.len(), 1);
        assert_eq!(parsed.models[0].name, "Sonnet");
        assert_eq!(parsed.models[0].pct, 45.0);
    }
}

