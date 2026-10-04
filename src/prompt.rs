use anyhow::Result;

use crate::aliases;
use crate::profiles;
use crate::usage;

pub fn print_prompt(short: bool, full: bool, is_json: bool) -> Result<()> {
    let current_opt = profiles::current_profile().unwrap_or(None);
    let Some(current) = current_opt else {
        if is_json {
            println!("null");
        }
        return Ok(());
    };

    let aliases_list = aliases::aliases_for_profile(&current).unwrap_or_default();
    let primary_alias = aliases_list.first().cloned();

    // Read purely from local disk cache, zero network calls
    let usage_opt = usage::get_profile_usage(&current, false).ok();

    if is_json {
        let (h5, d7) = match &usage_opt {
            Some(u) => (
                u.five_hour.as_ref().map(|h| h.pct),
                u.seven_day.as_ref().map(|d| d.pct),
            ),
            None => (None, None),
        };
        let out = serde_json::json!({
            "profile": current,
            "alias": primary_alias,
            "five_hour_pct": h5,
            "seven_day_pct": d7,
        });
        println!("{out}");
        return Ok(());
    }

    let display_name = match &primary_alias {
        Some(a) => format!("@{a}"),
        None => current.clone(),
    };

    if short {
        println!("{display_name}");
        return Ok(());
    }

    let mut quota_parts = Vec::new();
    if let Some(u) = &usage_opt
        && u.status == usage::UsageStatus::Ok
    {
        if let Some(h5) = &u.five_hour {
            quota_parts.push(format!("5h: {:.0}%", h5.pct));
        }
        if full && let Some(d7) = &u.seven_day {
            quota_parts.push(format!("7d: {:.0}%", d7.pct));
        }
    }

    if quota_parts.is_empty() {
        println!("[{display_name}]");
    } else {
        let prefix = if full { "⚡ " } else { "" };
        println!("{prefix}[{} {}]", display_name, quota_parts.join(" | "));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_prompt_does_not_panic() {
        let _ = print_prompt(false, false, false);
        let _ = print_prompt(true, false, false);
        let _ = print_prompt(false, true, false);
        let _ = print_prompt(false, false, true);
    }
}
