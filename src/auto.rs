use anyhow::{bail, Result};
use std::thread;
use std::time::Duration;

use crate::profiles;
use crate::usage::{self, UsageStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoStrategy {
    Best,
    NextAvailable,
    ConsumeFirst,
}

#[derive(Debug, Clone)]
pub struct AutoSwitchOptions {
    pub threshold: f64,
    pub strategy: AutoStrategy,
    pub model: Option<String>,
    pub interval_secs: u64,
    pub once: bool,
    pub dry_run: bool,
    pub json: bool,
    pub notify: bool,
}

impl Default for AutoSwitchOptions {
    fn default() -> Self {
        Self {
            threshold: 90.0,
            strategy: AutoStrategy::Best,
            model: None,
            interval_secs: 60,
            once: false,
            dry_run: false,
            json: false,
            notify: true,
        }
    }
}

pub enum Outcome {
    Switched,
    NothingToDo,
    Blocked,
}

struct Candidate {
    name: String,
    score: f64,
    seven_day_reset: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn parse_options(args: &[String]) -> Result<AutoSwitchOptions> {
    let settings = crate::settings::ToolSettings::load().unwrap_or_default();
    let mut opts = AutoSwitchOptions {
        threshold: settings.effective_threshold(),
        strategy: match settings.effective_strategy() {
            "next-available" | "next" => AutoStrategy::NextAvailable,
            "consume-first" => AutoStrategy::ConsumeFirst,
            _ => AutoStrategy::Best,
        },
        model: settings.effective_model(),
        interval_secs: settings.effective_interval(),
        once: false,
        dry_run: false,
        json: false,
        notify: true,
    };
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--threshold" => {
                if i + 1 < args.len() {
                    opts.threshold = args[i + 1].parse::<f64>()?;
                    i += 2;
                } else {
                    bail!("--threshold requires a numeric percentage argument");
                }
            }
            "--strategy" => {
                if i + 1 < args.len() {
                    match args[i + 1].to_lowercase().as_str() {
                        "best" => opts.strategy = AutoStrategy::Best,
                        "next-available" | "next" => opts.strategy = AutoStrategy::NextAvailable,
                        "consume-first" => opts.strategy = AutoStrategy::ConsumeFirst,
                        s => bail!("unknown strategy: {s}. Supported: best, next-available, consume-first"),
                    }
                    i += 2;
                } else {
                    bail!("--strategy requires an argument (best, next-available, consume-first)");
                }
            }
            "--model" => {
                if i + 1 < args.len() {
                    opts.model = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    bail!("--model requires a model name");
                }
            }
            "--interval" => {
                if i + 1 < args.len() {
                    opts.interval_secs = args[i + 1].parse::<u64>()?;
                    i += 2;
                } else {
                    bail!("--interval requires seconds");
                }
            }
            "--notify" => {
                opts.notify = true;
                i += 1;
            }
            "--no-notify" => {
                opts.notify = false;
                i += 1;
            }
            "--once" => {
                opts.once = true;
                i += 1;
            }
            "--dry-run" => {
                opts.dry_run = true;
                i += 1;
            }
            "--json" => {
                opts.json = true;
                i += 1;
            }
            arg if arg.starts_with("--threshold=") => {
                opts.threshold = arg.trim_start_matches("--threshold=").parse::<f64>()?;
                i += 1;
            }
            arg if arg.starts_with("--interval=") => {
                opts.interval_secs = arg.trim_start_matches("--interval=").parse::<u64>()?;
                i += 1;
            }
            arg if arg.starts_with("--strategy=") => {
                match arg.trim_start_matches("--strategy=").to_lowercase().as_str() {
                    "best" => opts.strategy = AutoStrategy::Best,
                    "next-available" | "next" => opts.strategy = AutoStrategy::NextAvailable,
                    "consume-first" => opts.strategy = AutoStrategy::ConsumeFirst,
                    s => bail!("unknown strategy: {s}"),
                }
                i += 1;
            }
            arg if arg.starts_with("--model=") => {
                opts.model = Some(arg.trim_start_matches("--model=").to_string());
                i += 1;
            }
            other => {
                bail!("unknown flag for auto: {other}");
            }
        }
    }

    Ok(opts)
}

pub fn run_auto(args: &[String]) -> Result<()> {
    if args.iter().any(|a| a == "--install-service") {
        return crate::service::install_service();
    }
    if args.iter().any(|a| a == "--uninstall-service") {
        return crate::service::uninstall_service();
    }
    if args.iter().any(|a| a == "--service-status") {
        return crate::service::service_status();
    }

    let opts = parse_options(args)?;

    if opts.once {
        let outcome = tick(&opts)?;
        let code = match outcome {
            Outcome::Switched => 0,
            Outcome::NothingToDo => 2,
            Outcome::Blocked => 3,
        };
        std::process::exit(code);
    }

    if !opts.json {
        let strat_name = match opts.strategy {
            AutoStrategy::Best => "best",
            AutoStrategy::NextAvailable => "next-available",
            AutoStrategy::ConsumeFirst => "consume-first",
        };
        eprintln!(
            "claude-user auto: monitoring active profile (threshold: {:.0}%, strategy: {}, interval: {}s)...",
            opts.threshold, strat_name, opts.interval_secs
        );
    }

    loop {
        match tick(&opts) {
            Ok(Outcome::Switched) => {
                // After switching, sleep before checking again
                thread::sleep(Duration::from_secs(opts.interval_secs));
            }
            Ok(Outcome::NothingToDo) | Ok(Outcome::Blocked) => {
                thread::sleep(Duration::from_secs(opts.interval_secs));
            }
            Err(e) => {
                if !opts.json {
                    eprintln!("auto-switch check error: {e}");
                }
                thread::sleep(Duration::from_secs(opts.interval_secs));
            }
        }
    }
}

pub fn tick(opts: &AutoSwitchOptions) -> Result<Outcome> {
    let current_name = match profiles::current_profile()? {
        Some(c) => c,
        None => bail!("no active profile set. Run `cuser <profile>` to launch or activate a profile first."),
    };

    let u = usage::get_profile_usage(&current_name, true)?;

    let mut needs_switch = false;
    let mut reason = String::new();

    if u.status == UsageStatus::RateLimited {
        needs_switch = true;
        reason = "Account is rate-limited (HTTP 429)".to_string();
    } else if u.status == UsageStatus::TokenExpired {
        needs_switch = true;
        reason = "OAuth token expired (re-login required)".to_string();
    } else if let Some(h5) = &u.five_hour
        && h5.pct >= opts.threshold
    {
        needs_switch = true;
        reason = format!(
            "5-hour limit exceeded threshold ({:.1}% >= {:.1}%)",
            h5.pct, opts.threshold
        );
    } else if let Some(d7) = &u.seven_day
        && d7.pct >= opts.threshold
    {
        needs_switch = true;
        reason = format!(
            "7-day limit exceeded threshold ({:.1}% >= {:.1}%)",
            d7.pct, opts.threshold
        );
    } else if let Some(target_model) = &opts.model {
        for m in &u.models {
            if m.name.eq_ignore_ascii_case(target_model) && m.pct >= opts.threshold {
                needs_switch = true;
                reason = format!(
                    "Model '{}' weekly limit exceeded threshold ({:.1}% >= {:.1}%)",
                    m.name, m.pct, opts.threshold
                );
                break;
            }
        }
    }

    if !needs_switch {
        if opts.json {
            println!(
                "{}",
                serde_json::json!({
                    "event": "status",
                    "active": current_name,
                    "action": "none",
                    "status": "healthy"
                })
            );
        } else if opts.once {
            println!(
                "Profile \"{current_name}\" is healthy (within threshold {:.0}%). No switch needed.",
                opts.threshold
            );
        }
        return Ok(Outcome::NothingToDo);
    }

    // Current account needs switching!
    let all = profiles::list_profiles()?;
    let mut candidates = Vec::new();

    for name in all {
        if name == current_name {
            continue;
        }
        if profiles::is_profile_disabled(&name).unwrap_or(false) {
            continue;
        }

        let cand_u = match usage::get_profile_usage(&name, false) {
            Ok(val) => val,
            Err(_) => continue,
        };

        if cand_u.status == UsageStatus::RateLimited || cand_u.status == UsageStatus::TokenExpired {
            continue;
        }

        if let Some(h5) = &cand_u.five_hour
            && h5.pct >= opts.threshold
        {
            continue;
        }
        if let Some(d7) = &cand_u.seven_day
            && d7.pct >= opts.threshold
        {
            continue;
        }

        if let Some(target_model) = &opts.model {
            let mut model_exhausted = false;
            for m in &cand_u.models {
                if m.name.eq_ignore_ascii_case(target_model) && m.pct >= opts.threshold {
                    model_exhausted = true;
                    break;
                }
            }
            if model_exhausted {
                continue;
            }
        }

        let score = cand_u
            .five_hour
            .as_ref()
            .map(|h| h.pct)
            .unwrap_or(0.0)
            .max(cand_u.seven_day.as_ref().map(|d| d.pct).unwrap_or(0.0));

        let reset_dt = cand_u
            .seven_day
            .as_ref()
            .and_then(|d| d.resets_at.as_deref())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        candidates.push(Candidate {
            name,
            score,
            seven_day_reset: reset_dt,
        });
    }

    if candidates.is_empty() {
        if opts.json {
            println!(
                "{}",
                serde_json::json!({
                    "event": "blocked",
                    "active": current_name,
                    "reason": reason,
                    "error": "all alternative accounts are exhausted, disabled, or rate-limited"
                })
            );
        } else {
            eprintln!(
                "Auto-switch: Active profile \"{current_name}\" requires rotation ({reason}), but all other accounts are exhausted or disabled."
            );
        }
        if opts.notify {
            crate::notify::send_notification(
                "Claude User: All Accounts Exhausted",
                &format!("Active profile \"{current_name}\" reached limit ({reason}), no alternatives available"),
            );
        }
        return Ok(Outcome::Blocked);
    }

    match opts.strategy {
        AutoStrategy::Best => {
            candidates.sort_by(|a, b| {
                a.score
                    .partial_cmp(&b.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        AutoStrategy::NextAvailable => {
            candidates.sort_by(|a, b| a.name.cmp(&b.name));
        }
        AutoStrategy::ConsumeFirst => {
            candidates.sort_by(|a, b| {
                match (&a.seven_day_reset, &b.seven_day_reset) {
                    (Some(ra), Some(rb)) => ra.cmp(rb).then_with(|| {
                        a.score
                            .partial_cmp(&b.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => a
                        .score
                        .partial_cmp(&b.score)
                        .unwrap_or(std::cmp::Ordering::Equal),
                }
            });
        }
    }

    let winner = &candidates[0];

    if !opts.dry_run {
        profiles::activate_profile(&winner.name)?;
        if opts.notify {
            crate::notify::send_notification(
                "Claude User Auto-Switch",
                &format!("Switched from \"{current_name}\" to \"{}\" ({reason})", winner.name),
            );
        }
    }

    if opts.json {
        println!(
            "{}",
            serde_json::json!({
                "event": "switch",
                "from": current_name,
                "to": winner.name,
                "reason": reason,
                "dry_run": opts.dry_run,
                "target_usage_pct": winner.score
            })
        );
    } else {
        let prefix = if opts.dry_run { "[dry-run] " } else { "" };
        println!(
            "{prefix}Auto-switch: Switched from \"{current_name}\" to \"{}\" ({reason}).",
            winner.name
        );
    }

    Ok(Outcome::Switched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_options() {
        let args = vec![
            "--threshold".to_string(),
            "80.5".to_string(),
            "--strategy".to_string(),
            "best".to_string(),
            "--model".to_string(),
            "Fable".to_string(),
            "--once".to_string(),
            "--dry-run".to_string(),
            "--json".to_string(),
        ];
        let opts = parse_options(&args).unwrap();
        assert_eq!(opts.threshold, 80.5);
        assert_eq!(opts.strategy, AutoStrategy::Best);
        assert_eq!(opts.model.as_deref(), Some("Fable"));
        assert!(opts.once);
        assert!(opts.dry_run);
        assert!(opts.json);
    }
}
