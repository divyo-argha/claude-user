use anyhow::{bail, Result};

use crate::aliases;
use crate::auto::AutoStrategy;
use crate::profiles;
use crate::usage::{self, UsageStatus};

pub fn run_switch(args: &[String]) -> Result<()> {
    let is_json = args.iter().any(|a| a == "--json");
    let is_force = args.iter().any(|a| a == "--force");

    let mut strategy: Option<AutoStrategy> = None;
    let mut target_profile: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" | "--force" => {
                i += 1;
            }
            "--strategy" => {
                if i + 1 < args.len() {
                    match args[i + 1].to_lowercase().as_str() {
                        "best" => strategy = Some(AutoStrategy::Best),
                        "next-available" | "next" => strategy = Some(AutoStrategy::NextAvailable),
                        "consume-first" => strategy = Some(AutoStrategy::ConsumeFirst),
                        s => bail!("unknown strategy: {s}. Supported: best, next-available, consume-first"),
                    }
                    i += 2;
                } else {
                    bail!("--strategy requires an argument (best, next-available, consume-first)");
                }
            }
            arg if arg.starts_with("--strategy=") => {
                match arg.trim_start_matches("--strategy=").to_lowercase().as_str() {
                    "best" => strategy = Some(AutoStrategy::Best),
                    "next-available" | "next" => strategy = Some(AutoStrategy::NextAvailable),
                    "consume-first" => strategy = Some(AutoStrategy::ConsumeFirst),
                    s => bail!("unknown strategy: {s}"),
                }
                i += 1;
            }
            val if !val.starts_with('-') && target_profile.is_none() => {
                target_profile = Some(val.to_string());
                i += 1;
            }
            other => {
                bail!("unknown flag for switch: {other}");
            }
        }
    }

    let current = profiles::current_profile()?;
    let all_profiles = profiles::list_profiles()?;

    if all_profiles.is_empty() {
        bail!("no profiles exist yet. Run `cuser <name>` to create one.");
    }

    // Determine target profile
    let winner = if let Some(target) = target_profile {
        if target == "next" {
            select_next_profile(&all_profiles, current.as_deref())?
        } else {
            // Resolve alias if any
            let resolved = aliases::resolve_profile_name(&target)?;
            if !profiles::profile_exists(&resolved)? {
                bail!("profile \"{target}\" does not exist");
            }
            resolved
        }
    } else if let Some(strat) = strategy {
        match strat {
            AutoStrategy::Best => select_best_quota_profile(&all_profiles, current.as_deref())?,
            AutoStrategy::NextAvailable => {
                select_next_available_profile(&all_profiles, current.as_deref())?
            }
            AutoStrategy::ConsumeFirst => {
                select_consume_first_profile(&all_profiles, current.as_deref())?
            }
        }
    } else {
        // Bare `cuser switch`: rotate to next profile
        select_next_profile(&all_profiles, current.as_deref())?
    };

    if current.as_deref() == Some(&winner) && !is_force {
        if is_json {
            println!(
                "{}",
                serde_json::json!({
                    "switched": false,
                    "from": current,
                    "to": winner,
                    "reason": "already active profile"
                })
            );
        } else {
            println!("Profile \"{winner}\" is already the active profile.");
        }
        return Ok(());
    }

    profiles::activate_profile(&winner)?;

    if is_json {
        println!(
            "{}",
            serde_json::json!({
                "switched": true,
                "from": current,
                "to": winner,
                "reason": "explicit switch"
            })
        );
    } else {
        println!("Switched active profile to \"{winner}\".");
    }

    Ok(())
}

fn select_next_profile(all: &[String], current: Option<&str>) -> Result<String> {
    let enabled: Vec<&String> = all
        .iter()
        .filter(|p| !profiles::is_profile_disabled(p).unwrap_or(false))
        .collect();

    if enabled.is_empty() {
        bail!("all profiles are disabled");
    }

    let Some(curr) = current else {
        return Ok(enabled[0].clone());
    };

    if let Some(pos) = enabled.iter().position(|p| p.as_str() == curr) {
        let next_idx = (pos + 1) % enabled.len();
        Ok(enabled[next_idx].clone())
    } else {
        Ok(enabled[0].clone())
    }
}

fn select_best_quota_profile(all: &[String], current: Option<&str>) -> Result<String> {
    let mut candidates = Vec::new();

    for p in all {
        if current == Some(p.as_str()) {
            continue;
        }
        if profiles::is_profile_disabled(p).unwrap_or(false) {
            continue;
        }

        let u = match usage::get_profile_usage(p, false) {
            Ok(u) => u,
            Err(_) => continue,
        };

        if u.status == UsageStatus::RateLimited || u.status == UsageStatus::TokenExpired {
            continue;
        }

        let score = u
            .five_hour
            .as_ref()
            .map(|h| h.pct)
            .unwrap_or(0.0)
            .max(u.seven_day.as_ref().map(|d| d.pct).unwrap_or(0.0));

        candidates.push((p.clone(), score));
    }

    if candidates.is_empty() {
        // Fall back to next profile if no usage candidate available
        return select_next_profile(all, current);
    }

    candidates.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(candidates[0].0.clone())
}

fn select_next_available_profile(all: &[String], current: Option<&str>) -> Result<String> {
    let enabled: Vec<&String> = all
        .iter()
        .filter(|p| !profiles::is_profile_disabled(p).unwrap_or(false))
        .collect();

    if enabled.is_empty() {
        bail!("all profiles are disabled");
    }

    let start_pos = match current {
        Some(curr) => enabled
            .iter()
            .position(|p| p.as_str() == curr)
            .map(|p| (p + 1) % enabled.len())
            .unwrap_or(0),
        None => 0,
    };

    for idx in 0..enabled.len() {
        let check_idx = (start_pos + idx) % enabled.len();
        let name = enabled[check_idx];

        if let Ok(u) = usage::get_profile_usage(name, false)
            && (u.status == UsageStatus::RateLimited || u.status == UsageStatus::TokenExpired)
        {
            continue;
        }

        return Ok(name.clone());
    }

    // Default to first enabled
    Ok(enabled[0].clone())
}

fn select_consume_first_profile(all: &[String], current: Option<&str>) -> Result<String> {
    let mut candidates = Vec::new();

    for p in all {
        if current == Some(p.as_str()) {
            continue;
        }
        if profiles::is_profile_disabled(p).unwrap_or(false) {
            continue;
        }

        let u = match usage::get_profile_usage(p, false) {
            Ok(u) => u,
            Err(_) => continue,
        };

        if u.status == UsageStatus::RateLimited || u.status == UsageStatus::TokenExpired {
            continue;
        }

        let score = u
            .five_hour
            .as_ref()
            .map(|h| h.pct)
            .unwrap_or(0.0)
            .max(u.seven_day.as_ref().map(|d| d.pct).unwrap_or(0.0));

        let reset_dt = u
            .seven_day
            .as_ref()
            .and_then(|d| d.resets_at.as_deref())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        candidates.push((p.clone(), reset_dt, score));
    }

    if candidates.is_empty() {
        return select_next_profile(all, current);
    }

    candidates.sort_by(|a, b| {
        match (&a.1, &b.1) {
            (Some(ra), Some(rb)) => ra.cmp(rb).then_with(|| {
                a.2.partial_cmp(&b.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal),
        }
    });

    Ok(candidates[0].0.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_next_profile() {
        let profiles = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(select_next_profile(&profiles, Some("a")).unwrap(), "b");
        assert_eq!(select_next_profile(&profiles, Some("b")).unwrap(), "c");
        assert_eq!(select_next_profile(&profiles, Some("c")).unwrap(), "a");
        assert_eq!(select_next_profile(&profiles, None).unwrap(), "a");
    }

    #[test]
    fn test_consume_first_sorting_prefers_soonest_reset() {
        let t1 = chrono::DateTime::parse_from_rfc3339("2026-06-22T10:00:00Z").unwrap().with_timezone(&chrono::Utc);
        let t2 = chrono::DateTime::parse_from_rfc3339("2026-06-25T10:00:00Z").unwrap().with_timezone(&chrono::Utc);

        let mut list = [
            ("later".to_string(), Some(t2), 20.0),
            ("sooner".to_string(), Some(t1), 50.0),
            ("none".to_string(), None, 10.0),
        ];

        list.sort_by(|a, b| {
            match (&a.1, &b.1) {
                (Some(ra), Some(rb)) => ra.cmp(rb).then_with(|| {
                    a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)
                }),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal),
            }
        });

        assert_eq!(list[0].0, "sooner");
        assert_eq!(list[1].0, "later");
        assert_eq!(list[2].0, "none");
    }
}
