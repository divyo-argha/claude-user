use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::oauth;
use crate::profiles;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

const USAGE_API_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CACHE_TTL_SECS: i64 = 300; // 5 minutes

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaceAnalysis {
    pub expected_pct: f64,
    pub pace_delta: f64,
    pub is_ahead: bool,
    pub is_under: bool,
    pub burn_rate_per_hour: f64,
    pub projected_hours_to_exhaustion: Option<f64>,
    pub will_last_to_reset: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowUsage {
    pub pct: f64,
    pub resets_at: Option<String>,
    pub countdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pace: Option<PaceAnalysis>,
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

pub fn is_no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

pub fn pct_color_ansi(pct: f64) -> (&'static str, &'static str) {
    let clamped = pct.clamp(0.0, 100.0);
    let code = if clamped >= 85.0 {
        "\x1b[38;2;248;113;113m" // Coral Red (#f87171)
    } else if clamped >= 60.0 {
        "\x1b[38;2;250;204;21m" // Amber / Yellow (#facc15)
    } else {
        "\x1b[38;2;74;222;128m" // Fresh Green (#4ade80)
    };
    (code, "\x1b[0m")
}

pub fn render_colored_progress_bar(pct: f64, width: usize) -> String {
    if is_no_color() {
        return render_progress_bar(pct, width);
    }
    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * (width as f64)).round() as usize;
    let empty = width.saturating_sub(filled);

    let (fg, reset) = pct_color_ansi(clamped);
    let bracket = "\x1b[38;2;100;116;139m"; // Slate 500
    let track = "\x1b[38;2;71;85;105m";     // Slate 600

    format!(
        "{bracket}[{reset}{fg}{}{reset}{track}{}{reset}{bracket}]{reset} {fg}{clamped:>3.0}%{reset}",
        "█".repeat(filled),
        "░".repeat(empty)
    )
}

pub fn format_countdown_cli(countdown: Option<&str>) -> String {
    match countdown {
        Some(c) => {
            if is_no_color() {
                format!(" (resets in {c})")
            } else {
                format!(" \x1b[38;2;56;189;248m(resets in {c})\x1b[0m")
            }
        }
        None => String::new(),
    }
}

pub fn progress_color_ratatui(pct: f64) -> Color {
    let clamped = pct.clamp(0.0, 100.0);
    if clamped >= 85.0 {
        Color::Rgb(248, 113, 113) // Red
    } else if clamped >= 60.0 {
        Color::Rgb(250, 204, 21) // Amber / Yellow
    } else {
        Color::Rgb(74, 222, 128) // Green
    }
}

pub fn render_tui_progress_spans(pct: f64, width: usize) -> Vec<Span<'static>> {
    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * (width as f64)).round() as usize;
    let empty = width.saturating_sub(filled);
    let color = progress_color_ratatui(clamped);

    let bracket_style = Style::default().fg(Color::Rgb(100, 116, 139));
    let filled_style = Style::default().fg(color);
    let empty_style = Style::default().fg(Color::Rgb(71, 85, 105));
    let text_style = Style::default().fg(color).add_modifier(Modifier::BOLD);

    vec![
        Span::styled("[", bracket_style),
        Span::styled("█".repeat(filled), filled_style),
        Span::styled("░".repeat(empty), empty_style),
        Span::styled("] ", bracket_style),
        Span::styled(format!("{:>3.0}%", clamped), text_style),
    ]
}

pub fn calculate_pace(pct: f64, resets_at: Option<&str>) -> Option<PaceAnalysis> {
    let resets_at = resets_at?;
    let dt = chrono::DateTime::parse_from_rfc3339(resets_at).ok()?;
    let now = Utc::now();
    let diff = dt.signed_duration_since(now);
    let rem_secs = diff.num_seconds();
    if rem_secs <= 0 {
        return None;
    }

    let rem_hours = (rem_secs as f64) / 3600.0;
    let total_window_hours = 168.0; // 7 days = 168 hours
    let clamped_rem_hours = rem_hours.min(total_window_hours);
    let elapsed_hours = (total_window_hours - clamped_rem_hours).max(0.5);

    let expected_pct = (elapsed_hours / total_window_hours) * 100.0;
    let pace_delta = pct - expected_pct;

    // Only flag ahead/under if at least 4 hours elapsed in window to avoid false positives
    let is_ahead = pace_delta > 10.0 && elapsed_hours >= 4.0;
    let is_under = pace_delta < -15.0 && elapsed_hours >= 4.0;

    let burn_rate_per_hour = (pct / elapsed_hours).max(0.0);
    let remaining_pct = (100.0 - pct).max(0.0);

    let (projected_hours_to_exhaustion, will_last_to_reset) = if burn_rate_per_hour > 0.001 {
        let hrs = remaining_pct / burn_rate_per_hour;
        let will_last = hrs >= clamped_rem_hours;
        (Some(hrs), will_last)
    } else {
        (None, true)
    };

    Some(PaceAnalysis {
        expected_pct,
        pace_delta,
        is_ahead,
        is_under,
        burn_rate_per_hour,
        projected_hours_to_exhaustion,
        will_last_to_reset,
    })
}

pub fn format_pace_cli(pace: &PaceAnalysis) -> String {
    let badge = if pace.is_ahead {
        if is_no_color() {
            format!("🔥 Ahead of pace (+{:.0}%)", pace.pace_delta)
        } else {
            format!("\x1b[38;2;248;113;113m🔥 Ahead of pace (+{:.0}%)\x1b[0m", pace.pace_delta)
        }
    } else if pace.is_under {
        if is_no_color() {
            format!("✨ Under pace ({:.0}%)", pace.pace_delta)
        } else {
            format!("\x1b[38;2;56;189;248m✨ Under pace ({:.0}%)\x1b[0m", pace.pace_delta)
        }
    } else {
        if is_no_color() {
            format!("🌱 On track (exp ~{:.0}%)", pace.expected_pct)
        } else {
            format!("\x1b[38;2;74;222;128m🌱 On track (exp ~{:.0}%)\x1b[0m", pace.expected_pct)
        }
    };

    let burn = if is_no_color() {
        format!("Burn: {:.1}%/h", pace.burn_rate_per_hour)
    } else {
        format!("\x1b[38;2;148;163;184mBurn: {:.1}%/h\x1b[0m", pace.burn_rate_per_hour)
    };

    let proj = if !pace.will_last_to_reset && let Some(hrs) = pace.projected_hours_to_exhaustion {
        if is_no_color() {
            format!("⚠️ May exhaust in ~{:.0}h", hrs)
        } else {
            format!("\x1b[38;2;250;204;21m⚠️ May exhaust in ~{:.0}h\x1b[0m", hrs)
        }
    } else {
        if is_no_color() {
            "✔ Lasts to reset".to_string()
        } else {
            "\x1b[38;2;74;222;128m✔ Lasts to reset\x1b[0m".to_string()
        }
    };

    format!("{badge} • {burn} • {proj}")
}

pub fn render_pace_tui_spans(pace: &PaceAnalysis) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if pace.is_ahead {
        spans.push(Span::styled("🔥 Ahead of pace ", Style::default().fg(Color::Rgb(248, 113, 113)).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!("(+{:.0}%) ", pace.pace_delta), Style::default().fg(Color::Rgb(248, 113, 113))));
    } else if pace.is_under {
        spans.push(Span::styled("✨ Under pace ", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!("({:.0}%) ", pace.pace_delta), Style::default().fg(Color::Rgb(56, 189, 248))));
    } else {
        spans.push(Span::styled("🌱 On track ", Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!("(exp ~{:.0}%) ", pace.expected_pct), Style::default().fg(Color::Rgb(148, 163, 184))));
    }

    spans.push(Span::styled("• ", Style::default().fg(Color::Rgb(100, 116, 139))));
    spans.push(Span::styled(format!("{:.1}%/h burn ", pace.burn_rate_per_hour), Style::default().fg(Color::Rgb(148, 163, 184))));
    spans.push(Span::styled("• ", Style::default().fg(Color::Rgb(100, 116, 139))));

    if !pace.will_last_to_reset && let Some(hrs) = pace.projected_hours_to_exhaustion {
        spans.push(Span::styled(format!("⚠️ May exhaust in ~{:.0}h", hrs), Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)));
    } else {
        spans.push(Span::styled("✔ Lasts to reset", Style::default().fg(Color::Rgb(74, 222, 128))));
    }

    spans
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

    let previous = cache.get(profile).cloned();
    let mut usage = fetch_profile_usage_live(profile)?;

    // If live fetch was throttled (429) or unavailable, retain previous known quota limits
    if (usage.status == UsageStatus::RateLimited || usage.status == UsageStatus::Unavailable)
        && let Some(prev) = &previous
        && prev.five_hour.is_some()
    {
        usage.five_hour = prev.five_hour.clone();
        usage.seven_day = prev.seven_day.clone();
        usage.spend = prev.spend.clone();
        usage.models = prev.models.clone();
    }

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
            pace: None,
        })
    });

    let seven_day = data.get("seven_day").and_then(|d7| {
        let pct = d7.get("utilization")?.as_f64()?;
        let resets_at = d7.get("resets_at").and_then(|r| r.as_str()).map(String::from);
        let countdown = resets_at.as_deref().and_then(format_reset_countdown);
        let pace = calculate_pace(pct, resets_at.as_deref());
        Some(WindowUsage {
            pct,
            resets_at,
            countdown,
            pace,
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
    fn test_render_colored_progress_bar() {
        let bar = render_colored_progress_bar(40.0, 10);
        assert!(bar.contains("████"));
        assert!(bar.contains("░░░░░░"));
        assert!(bar.contains("40%"));

        let bar_amber = render_colored_progress_bar(70.0, 10);
        assert!(bar_amber.contains("███████"));
        assert!(bar_amber.contains("70%"));

        let bar_red = render_colored_progress_bar(90.0, 10);
        assert!(bar_red.contains("█████████"));
        assert!(bar_red.contains("90%"));

        if !is_no_color() {
            assert!(bar.contains("\x1b[38;2;74;222;128m"));
            assert!(bar_amber.contains("\x1b[38;2;250;204;21m"));
            assert!(bar_red.contains("\x1b[38;2;248;113;113m"));
        }
    }

    #[test]
    fn test_render_tui_progress_spans() {
        let spans_green = render_tui_progress_spans(30.0, 10);
        assert_eq!(spans_green.len(), 5);
        assert_eq!(spans_green[0].content, "[");
        assert_eq!(spans_green[1].content, "███");
        assert_eq!(spans_green[1].style.fg, Some(Color::Rgb(74, 222, 128)));
        assert_eq!(spans_green[2].content, "░░░░░░░");
        assert_eq!(spans_green[3].content, "] ");
        assert_eq!(spans_green[4].content, " 30%");

        let spans_amber = render_tui_progress_spans(65.0, 10);
        assert_eq!(spans_amber[1].style.fg, Some(Color::Rgb(250, 204, 21)));

        let spans_red = render_tui_progress_spans(92.0, 10);
        assert_eq!(spans_red[1].style.fg, Some(Color::Rgb(248, 113, 113)));
    }

    #[test]
    fn test_format_countdown_cli() {
        assert_eq!(format_countdown_cli(None), "");
        let rst = format_countdown_cli(Some("1h 30m"));
        assert!(rst.contains("1h 30m"));
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

    #[test]
    fn test_calculate_pace() {
        assert_eq!(calculate_pace(50.0, None), None);
        // Past date
        assert_eq!(calculate_pace(50.0, Some("2020-01-01T00:00:00Z")), None);

        // Future date (e.g. 7 days from now)
        let future = (Utc::now() + chrono::Duration::hours(100)).to_rfc3339();
        let pace = calculate_pace(75.0, Some(&future));
        assert!(pace.is_some());
        let p = pace.unwrap();
        assert!(p.burn_rate_per_hour > 0.0);
        let cli_str = format_pace_cli(&p);
        assert!(!cli_str.is_empty());
        let tui_spans = render_pace_tui_spans(&p);
        assert!(!tui_spans.is_empty());
    }
}

