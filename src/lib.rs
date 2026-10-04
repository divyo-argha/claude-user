pub mod launch;
pub mod mappings;
pub mod oauth;
pub mod profiles;
pub mod tui;
pub mod update;
pub mod usage;

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::io::{self, Write};
use std::path::PathBuf;

const HELP: &str = "\
claude-user — switch between Claude accounts (alias: cuser)

USAGE:
    claude-user                    open the interactive profile picker with live quota
    claude-user <profile>          launch that profile directly (created if new)
    claude-user <profile> [args]   launch that profile, passing [args] to `claude`
    claude-user run [args]         launch profile mapped to current directory
    claude-user list | -l          list existing profiles with 5h/7d usage limits
    claude-user usage [profile]    show detailed quota and rate limit breakdown
    claude-user current            show the profile currently pointed to by `claude`
    claude-user map [profile] [dir] bind a directory to a profile (or list mappings)
    claude-user unmap [dir]        remove a directory mapping
    claude-user disable <profile>  hold a profile out of rotation
    claude-user enable <profile>   re-enable a disabled profile
    claude-user sync               copy shared config into every existing profile
    claude-user import [name]      import your currently logged-in ~/.claude as a new profile
    claude-user remove <profile>   delete a profile (asks for confirmation)
    claude-user rename <old> <new> rename a profile
    claude-user --update           update claude-user to the latest release
    claude-user --version | -v     show the installed version
    claude-user --help | -h        show this help

FLAGS:
    --json                         output results in machine-readable JSON
    --refresh                      bypass 5-minute cache and fetch live usage from API

The picker (plain `claude-user` / `cuser`) also offers \"+ Import ~/.claude\" whenever a
default ~/.claude exists, and \"+ New profile\" to log into a brand-new account.
Highlighting an existing profile in the picker also offers `d` to delete it,
`r` to rename it, and `e` to toggle disabling/enabling it.

Launching a profile also points ~/.claude and ~/.claude.json at it (via
symlinks), so `claude` run directly afterward uses that same account. If
~/.claude already exists as a real directory, run `claude-user import <name>` first.
";

#[derive(Serialize)]
struct ProfileListEntry {
    name: String,
    is_current: bool,
    is_disabled: bool,
    email: Option<String>,
    org_name: Option<String>,
    mapped_paths: Vec<String>,
    usage: Option<usage::AccountUsage>,
}

#[derive(Serialize)]
struct ListOutput {
    current: Option<String>,
    profiles: Vec<ProfileListEntry>,
}

#[derive(Serialize)]
struct CurrentOutput {
    current: Option<String>,
    is_disabled: bool,
    email: Option<String>,
    org_name: Option<String>,
    mapped_to_cwd: bool,
    cwd: String,
    usage: Option<usage::AccountUsage>,
}

pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        return run_picker();
    }

    let is_json = args.iter().any(|a| a == "--json");
    let force_refresh = args.iter().any(|a| a == "--refresh");

    match args[0].as_str() {
        "--help" | "-h" | "help" => {
            print!("{HELP}");
            Ok(())
        }
        "--version" | "-v" => {
            println!("claude-user {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "list" | "-l" => cmd_list(is_json, force_refresh),
        "usage" | "quota" => {
            let filtered: Vec<String> = args
                .iter()
                .skip(1)
                .filter(|a| *a != "--json" && *a != "--refresh")
                .cloned()
                .collect();
            cmd_usage(&filtered, is_json, force_refresh)
        }
        "current" | "active" | "status" => cmd_current(is_json, force_refresh),
        "run" => cmd_run(&args[1..]),
        "map" => {
            let filtered: Vec<String> = args
                .iter()
                .skip(1)
                .filter(|a| *a != "--json" && *a != "--refresh")
                .cloned()
                .collect();
            cmd_map(&filtered, is_json)
        }
        "unmap" => cmd_unmap(args.get(1).cloned()),
        "disable" => cmd_disable(args.get(1).cloned()),
        "enable" => cmd_enable(args.get(1).cloned()),
        "sync" => cmd_sync(),
        "import" | "migrate" => cmd_import(args.get(1).cloned()),
        "remove" | "rm" | "delete" => cmd_remove(args.get(1).cloned()),
        "rename" => cmd_rename(args.get(1).cloned(), args.get(2).cloned()),
        "--update" | "update" => update::run(),
        "--json" => cmd_list(true, force_refresh),
        name => {
            profiles::validate_profile_name(name)?;
            if !profiles::profile_exists(name)? {
                profiles::check_default_available()?;
            }
            let created = profiles::ensure_profile(name)?;
            if created {
                eprintln!("Creating new profile: {name}");
            }
            launch_profile(name, &args[1..])
        }
    }
}

fn run_picker() -> Result<()> {
    match tui::run_picker()? {
        Some(tui::PickResult::Existing(name)) => launch_profile(&name, &[]),
        Some(tui::PickResult::New(name)) => {
            profiles::check_default_available()?;
            profiles::create_profile(&name)?;
            eprintln!("Creating new profile: {name}");
            launch_profile(&name, &[])
        }
        Some(tui::PickResult::Import(name)) => {
            profiles::import_default(&name)?;
            println!("Imported ~/.claude as profile \"{name}\".");
            launch_profile(&name, &[])
        }
        None => Ok(()),
    }
}

fn launch_profile(name: &str, args: &[String]) -> Result<()> {
    if profiles::is_profile_disabled(name).unwrap_or(false) {
        eprintln!("Note: profile \"{name}\" is marked as disabled.");
    }
    profiles::activate_profile(name)?;
    let dir = profiles::profile_dir(name)?;
    launch::launch_claude(&dir, args)
}

fn cmd_run(args: &[String]) -> Result<()> {
    let cwd = std::env::current_dir()?;
    if let Some((profile, matched_dir)) = mappings::resolve_mapping(&cwd)? {
        eprintln!(
            "Using profile \"{profile}\" (mapped from {})",
            matched_dir.display()
        );
        launch_profile(&profile, args)
    } else if let Some(current) = profiles::current_profile()? {
        eprintln!(
            "No mapping for {}. Using current profile \"{current}\".",
            cwd.display()
        );
        launch_profile(&current, args)
    } else {
        bail!(
            "no directory mapping found for {}, and no active profile is set.\nRun `cuser map <profile>` to map this directory, or `cuser <profile>` to launch.",
            cwd.display()
        );
    }
}

fn cmd_usage(args: &[String], is_json: bool, force_refresh: bool) -> Result<()> {
    let target_profile = args
        .first()
        .cloned()
        .or_else(|| profiles::current_profile().ok().flatten());

    let Some(profile) = target_profile else {
        bail!("no profile specified and no active profile is set.\nUsage: `cuser usage <profile>` or `cuser list`");
    };

    if !profiles::profile_exists(&profile)? {
        bail!("profile \"{profile}\" does not exist");
    }

    let u = usage::get_profile_usage(&profile, force_refresh)?;

    if is_json {
        println!("{}", serde_json::to_string_pretty(&u)?);
        return Ok(());
    }

    let info = profiles::get_profile_info(&profile)?;
    let email_part = match (info.email, info.org_name) {
        (Some(e), Some(o)) => format!(" ({e} • {o})"),
        (Some(e), None) => format!(" ({e})"),
        _ => String::new(),
    };

    println!("Profile: {profile}{email_part}");
    match u.status {
        usage::UsageStatus::Ok => {
            println!("\nQuota & Rate Limits:");
            if let Some(h5) = &u.five_hour {
                let bar = usage::render_progress_bar(h5.pct, 20);
                let rst = h5
                    .countdown
                    .as_deref()
                    .map(|c| format!(" (resets in {c})"))
                    .unwrap_or_default();
                println!("  5-Hour Limit:  {bar}{rst}");
            }
            if let Some(d7) = &u.seven_day {
                let bar = usage::render_progress_bar(d7.pct, 20);
                let rst = d7
                    .countdown
                    .as_deref()
                    .map(|c| format!(" (resets in {c})"))
                    .unwrap_or_default();
                println!("  7-Day Limit:   {bar}{rst}");
            }
            if let Some(sp) = &u.spend {
                let bar = usage::render_progress_bar(sp.pct, 20);
                println!(
                    "  Extra Spend:   {bar} (${:.2} used of ${:.2} limit)",
                    sp.used, sp.limit
                );
            }
            if !u.models.is_empty() {
                println!("\nPer-Model Weekly Limits:");
                for m in &u.models {
                    let bar = usage::render_progress_bar(m.pct, 20);
                    let rst = m
                        .countdown
                        .as_deref()
                        .map(|c| format!(" (resets in {c})"))
                        .unwrap_or_default();
                    println!("  {:<14} {bar}{rst}", m.name);
                }
            }
        }
        usage::UsageStatus::RateLimited => {
            println!("  Status: Rate-limited on Anthropic usage endpoint (429). Retrying later.");
        }
        usage::UsageStatus::TokenExpired => {
            println!("  Status: OAuth token expired. Launch `cuser {profile}` to renew login.");
        }
        usage::UsageStatus::NoUsageAccess => {
            println!("  Status: Account tier or organization does not expose OAuth usage endpoint.");
        }
        usage::UsageStatus::Unavailable => {
            let msg = u.error_message.as_deref().unwrap_or("unknown error");
            println!("  Status: Unavailable ({msg})");
        }
    }

    Ok(())
}

fn cmd_map(args: &[String], is_json: bool) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if args.is_empty() {
        let all_mappings = mappings::list_mappings()?;
        let active_mapping = mappings::resolve_mapping(&cwd)?;

        if is_json {
            let active_entry = active_mapping.map(|(profile, path)| mappings::MapListEntry {
                path: path.to_string_lossy().into_owned(),
                profile,
            });
            let mappings_entries = all_mappings
                .into_iter()
                .map(|(path, profile)| mappings::MapListEntry {
                    path: path.to_string_lossy().into_owned(),
                    profile,
                })
                .collect();
            let output = mappings::MapOutput {
                cwd: cwd.to_string_lossy().into_owned(),
                active_mapping: active_entry,
                mappings: mappings_entries,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }

        if all_mappings.is_empty() {
            println!("No directory mappings configured yet.");
            println!("Run `cuser map <profile> [dir]` to map a directory.");
            return Ok(());
        }

        println!("Directory Mappings:");
        for (path, profile) in all_mappings {
            println!("  {} -> {profile}", path.display());
        }
        println!();
        println!("Current directory: {}", cwd.display());
        if let Some((profile, matched_path)) = active_mapping {
            println!("Mapped profile:    {profile} (via {})", matched_path.display());
        } else {
            println!("Mapped profile:    (none)");
        }
        return Ok(());
    }

    let profile = &args[0];
    let dir_arg = args.get(1).map(|s| s.as_str());
    let path = mappings::add_mapping(profile, dir_arg)?;
    println!("Mapped directory {} to profile \"{profile}\".", path.display());
    Ok(())
}

fn cmd_unmap(dir_arg: Option<String>) -> Result<()> {
    let (path, prev) = mappings::remove_mapping(dir_arg.as_deref())?;
    match prev {
        Some(profile) => println!(
            "Unmapped directory {} (was mapped to \"{profile}\").",
            path.display()
        ),
        None => println!("No mapping found for {}.", path.display()),
    }
    Ok(())
}

fn cmd_disable(name: Option<String>) -> Result<()> {
    let name = name.ok_or_else(|| anyhow!("usage: cuser disable <profile>"))?;
    profiles::set_profile_disabled(&name, true)?;
    let current = profiles::current_profile()?;
    if current.as_deref() == Some(&name) {
        println!("Profile \"{name}\" is now disabled (note: it is currently the active profile).");
    } else {
        println!("Profile \"{name}\" is now disabled.");
    }
    Ok(())
}

fn cmd_enable(name: Option<String>) -> Result<()> {
    let name = name.ok_or_else(|| anyhow!("usage: cuser enable <profile>"))?;
    profiles::set_profile_disabled(&name, false)?;
    println!("Profile \"{name}\" is now enabled.");
    Ok(())
}

fn cmd_list(is_json: bool, force_refresh: bool) -> Result<()> {
    let names = profiles::list_profiles()?;
    let current = profiles::current_profile()?;

    if is_json {
        let mut list = Vec::new();
        for name in &names {
            let info = profiles::get_profile_info(name)?;
            let mapped_paths: Vec<String> = mappings::mappings_for_profile(name)?
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let u = usage::get_profile_usage(name, force_refresh).ok();
            list.push(ProfileListEntry {
                name: name.clone(),
                is_current: current.as_deref() == Some(name),
                is_disabled: info.is_disabled,
                email: info.email,
                org_name: info.org_name,
                mapped_paths,
                usage: u,
            });
        }
        let out = ListOutput {
            current,
            profiles: list,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if names.is_empty() {
        println!("No profiles yet. Run `cuser <name>` to create one.");
    } else {
        for name in names {
            let is_current = current.as_deref() == Some(&name);
            let badge = if is_current { "  * (current)" } else { "" };
            let info = profiles::get_profile_info(&name)?;
            let disabled_badge = if info.is_disabled { " [disabled]" } else { "" };
            match (info.email, info.org_name) {
                (Some(email), Some(org)) => {
                    println!("{name}  ({email} • {org}){disabled_badge}{badge}")
                }
                (Some(email), None) => println!("{name}  ({email}){disabled_badge}{badge}"),
                (None, _) => println!("{name}{disabled_badge}{badge}"),
            }

            if let Ok(u) = usage::get_profile_usage(&name, force_refresh) {
                match u.status {
                    usage::UsageStatus::Ok => {
                        if let Some(h5) = &u.five_hour {
                            let bar = usage::render_progress_bar(h5.pct, 16);
                            let rst = h5
                                .countdown
                                .as_deref()
                                .map(|c| format!(" (resets in {c})"))
                                .unwrap_or_default();
                            println!("    5h quota:  {bar}{rst}");
                        }
                        if let Some(d7) = &u.seven_day {
                            let bar = usage::render_progress_bar(d7.pct, 16);
                            let rst = d7
                                .countdown
                                .as_deref()
                                .map(|c| format!(" (resets in {c})"))
                                .unwrap_or_default();
                            println!("    7d quota:  {bar}{rst}");
                        }
                    }
                    usage::UsageStatus::TokenExpired => {
                        println!("    usage:     [token expired - run `cuser {name}` to renew]");
                    }
                    usage::UsageStatus::RateLimited => {
                        println!("    usage:     [rate-limited (429) - retrying later]");
                    }
                    usage::UsageStatus::NoUsageAccess => {
                        println!("    usage:     [OAuth usage API not supported for this account tier]");
                    }
                    usage::UsageStatus::Unavailable => {
                        if let Some(err) = &u.error_message {
                            println!("    usage:     [{err}]");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn cmd_current(is_json: bool, force_refresh: bool) -> Result<()> {
    let current = profiles::current_profile()?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cwd_str = cwd.to_string_lossy().into_owned();

    let mapped_to_cwd = if let Some(ref curr) = current {
        mappings::resolve_mapping(&cwd)?
            .map(|(p, _)| p == *curr)
            .unwrap_or(false)
    } else {
        false
    };

    let usage_val = if let Some(ref name) = current {
        usage::get_profile_usage(name, force_refresh).ok()
    } else {
        None
    };

    if is_json {
        let (is_disabled, email, org_name) = if let Some(ref name) = current {
            let info = profiles::get_profile_info(name)?;
            (info.is_disabled, info.email, info.org_name)
        } else {
            (false, None, None)
        };

        let out = CurrentOutput {
            current,
            is_disabled,
            email,
            org_name,
            mapped_to_cwd,
            cwd: cwd_str,
            usage: usage_val,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    match current {
        Some(name) => {
            let info = profiles::get_profile_info(&name)?;
            let disabled_note = if info.is_disabled { " [disabled]" } else { "" };
            let details = match (info.email, info.org_name) {
                (Some(email), Some(org)) => format!(" ({email} • {org})"),
                (Some(email), None) => format!(" ({email})"),
                (None, _) => String::new(),
            };
            println!("{name}{details}{disabled_note}");

            if let Some(u) = usage_val
                && u.status == usage::UsageStatus::Ok
            {
                if let Some(h5) = &u.five_hour {
                    let bar = usage::render_progress_bar(h5.pct, 16);
                    let rst = h5
                        .countdown
                        .as_deref()
                        .map(|c| format!(" (resets in {c})"))
                        .unwrap_or_default();
                    println!("  5h quota:  {bar}{rst}");
                }
                if let Some(d7) = &u.seven_day {
                    let bar = usage::render_progress_bar(d7.pct, 16);
                    let rst = d7
                        .countdown
                        .as_deref()
                        .map(|c| format!(" (resets in {c})"))
                        .unwrap_or_default();
                    println!("  7d quota:  {bar}{rst}");
                }
            }
        }
        None => {
            println!("No active profile (running `claude` in terminal uses default ~/.claude).");
        }
    }
    Ok(())
}

fn cmd_sync() -> Result<()> {
    let names = profiles::sync_all()?;
    if names.is_empty() {
        println!("No profiles to sync yet.");
    } else {
        println!("Synced shared config into: {}", names.join(", "));
    }
    Ok(())
}

fn cmd_import(name: Option<String>) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => prompt("Name for this account (e.g. \"main\"): ")?.trim().to_string(),
    };
    profiles::import_default(&name)?;
    println!("Imported ~/.claude as profile \"{name}\".");
    Ok(())
}

fn cmd_remove(name: Option<String>) -> Result<()> {
    let name = name.ok_or_else(|| anyhow!("usage: cuser remove <profile>"))?;
    if !profiles::profile_exists(&name)? {
        bail!("profile \"{name}\" does not exist");
    }
    let answer = prompt(&format!(
        "This deletes ~/.claude-profiles/{name}, including its stored login. Continue? [y/N] "
    ))?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        println!("Cancelled.");
        return Ok(());
    }
    profiles::remove_profile(&name)?;
    println!("Removed profile \"{name}\".");
    Ok(())
}

fn cmd_rename(old: Option<String>, new: Option<String>) -> Result<()> {
    let old = old.ok_or_else(|| anyhow!("usage: cuser rename <old> <new>"))?;
    let new = new.ok_or_else(|| anyhow!("usage: cuser rename <old> <new>"))?;
    profiles::rename_profile(&old, &new)?;
    println!("Renamed \"{old}\" to \"{new}\".");
    Ok(())
}

fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input)
}
