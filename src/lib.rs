pub mod aliases;
pub mod auto;
pub mod completions;
pub mod doctor;
pub mod launch;
pub mod mappings;
pub mod notify;
pub mod oauth;
pub mod profiles;
pub mod prompt;
pub mod service;
pub mod settings;
pub mod switch;
pub mod token;
pub mod transfer;
pub mod tui;
pub mod update;
pub mod usage;
pub mod watch;

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::io::{self, Read, Write};
use std::path::PathBuf;

const HELP: &str = "\
claude-user — switch between Claude accounts (alias: cuser)

USAGE:
    claude-user                    open the interactive profile picker with live quota
    claude-user <profile>          launch that profile directly (created if new)
    claude-user <profile> [args]   launch that profile, passing [args] to `claude`
    claude-user switch [profile]   switch active profile without launching (or --strategy best)
    claude-user run [prof] [args]  launch in isolated session mode (or directory-mapped profile)
    claude-user list | -l          list existing profiles with 5h/7d usage limits
    claude-user usage [profile]    show detailed quota and rate limit breakdown
    claude-user watch [interval]   live auto-refreshing quota monitor dashboard
    claude-user prompt [flags]     zero-latency status for shell prompts (Starship/zsh/tmux)
    claude-user auto [flags]       auto-rotate accounts before hitting rate limits
    claude-user doctor [--fix]     diagnose CLI, tokens, symlinks, permissions & auto-repair
    claude-user alias [prof] [ali] assign short alias to a profile (or list all aliases)
    claude-user unalias <alias>    remove a profile alias
    claude-user config [get|set]   view and edit tool settings (e.g. autoswitch)
    claude-user add-token <t> <p>  register profile from OAuth setup token or API key
    claude-user export [file]      export profiles and credentials to a backup JSON
    claude-user import [file|name] import backup file or import ~/.claude as profile
    claude-user import-usage <f>   import usage snapshot readings from another machine
    claude-user current            show the profile currently pointed to by `claude`
    claude-user map [profile] [dir] bind a directory to a profile (or list mappings)
    claude-user unmap [dir]        remove a directory mapping
    claude-user disable <profile>  hold a profile out of rotation
    claude-user enable <profile>   re-enable a disabled profile
    claude-user sync               copy shared config into every existing profile
    claude-user remove <profile>   delete a profile (asks for confirmation)
    claude-user rename <old> <new> rename a profile
    claude-user purge              remove all claude-user data and profiles
    claude-user completions <sh>   generate shell completion script (bash, zsh, fish, pwsh)
    claude-user --update           update claude-user to the latest release
    claude-user --version | -v     show the installed version
    claude-user --help | -h        show this help

FLAGS:
    --json                         output results in machine-readable JSON
    --refresh                      bypass 5-minute cache and fetch live usage from API
    --token-status                 include token and credential diagnostics in `list`
    --threshold <pct>              quota threshold for auto-switch (default: 90)
    --strategy <best|next|consume-first> auto-switch selection strategy (default: best)
    --model <name>                 also switch when per-model limit exceeds threshold
    --interval <secs>              polling interval in seconds (default: 60)
    --once                         run auto-switch once and exit with status code
    --dry-run                      simulate auto-switch without activating profile
    --force                        overwrite existing accounts during import

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
    aliases: Vec<String>,
    is_current: bool,
    is_disabled: bool,
    email: Option<String>,
    org_name: Option<String>,
    mapped_paths: Vec<String>,
    usage: Option<usage::AccountUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_status: Option<oauth::TokenStatusSummary>,
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
    let token_status = args.iter().any(|a| a == "--token-status");

    match args[0].as_str() {
        "--help" | "-h" | "help" => {
            print!("{HELP}");
            Ok(())
        }
        "--version" | "-v" => {
            println!("claude-user {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "list" | "-l" => cmd_list(is_json, force_refresh, token_status),
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
        "switch" => switch::run_switch(&args[1..]),
        "alias" => {
            let filtered: Vec<String> = args
                .iter()
                .skip(1)
                .filter(|a| *a != "--json")
                .cloned()
                .collect();
            cmd_alias(&filtered, is_json)
        }
        "unalias" => cmd_unalias(args.get(1).cloned()),
        "config" => cmd_config(&args[1..], is_json),
        "import-usage" => cmd_import_usage(&args[1..]),
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
        "auto" => auto::run_auto(&args[1..]),
        "add-token" => cmd_add_token(&args[1..]),
        "export" => cmd_export(&args[1..]),
        "import" | "migrate" => cmd_import(&args[1..]),
        "remove" | "rm" | "delete" => cmd_remove(args.get(1).cloned()),
        "rename" => cmd_rename(args.get(1).cloned(), args.get(2).cloned()),
        "watch" | "top" => {
            let interval = args
                .iter()
                .skip(1)
                .find_map(|a| a.parse::<u64>().ok())
                .unwrap_or(10);
            watch::run_watch(interval)
        }
        "prompt" => {
            let short = args.iter().any(|a| a == "--short" || a == "-s");
            let full = args.iter().any(|a| a == "--full");
            prompt::print_prompt(short, full, is_json)
        }
        "doctor" | "check" => {
            let auto_fix = args.iter().any(|a| a == "--fix");
            doctor::run_doctor(auto_fix)
        }
        "purge" => cmd_purge(),
        "completions" => completions::print_completions(args.get(1).map(|s| s.as_str()).unwrap_or("")),
        "--update" | "update" => update::run(),
        "--json" => cmd_list(true, force_refresh, token_status),
        name => {
            let resolved = aliases::resolve_profile_name(name)?;
            profiles::validate_profile_name(&resolved)?;
            if !profiles::profile_exists(&resolved)? {
                profiles::check_default_available()?;
            }
            let created = profiles::ensure_profile(&resolved)?;
            if created {
                eprintln!("Creating new profile: {resolved}");
            }
            launch_profile(&resolved, &args[1..])
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

fn launch_session_profile(name: &str, args: &[String]) -> Result<()> {
    if profiles::is_profile_disabled(name).unwrap_or(false) {
        eprintln!("Note: profile \"{name}\" is marked as disabled.");
    }
    let dir = profiles::profile_dir(name)?;
    launch::launch_claude(&dir, args)
}

fn cmd_run(args: &[String]) -> Result<()> {
    // 1. If explicit profile argument or alias provided, launch in session mode (no symlink mutation)
    if let Some(first) = args.first() {
        let resolved = aliases::resolve_profile_name(first).unwrap_or_else(|_| first.clone());
        if profiles::profile_exists(&resolved).unwrap_or(false) {
            eprintln!("Running Claude Code as \"{resolved}\" in isolated session mode...");
            return launch_session_profile(&resolved, &args[1..]);
        }
    }

    // 2. Otherwise resolve directory mapping
    let cwd = std::env::current_dir()?;
    if let Some((profile, matched_dir)) = mappings::resolve_mapping(&cwd)? {
        eprintln!(
            "Using profile \"{profile}\" (mapped from {})",
            matched_dir.display()
        );
        launch_session_profile(&profile, args)
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

    let Some(raw_profile) = target_profile else {
        bail!("no profile specified and no active profile is set.\nUsage: `cuser usage <profile>` or `cuser list`");
    };

    let profile = aliases::resolve_profile_name(&raw_profile)?;

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
                let bar = usage::render_colored_progress_bar(h5.pct, 20);
                let rst = usage::format_countdown_cli(h5.countdown.as_deref());
                println!("  5-Hour Limit:  {bar}{rst}");
            }
            if let Some(d7) = &u.seven_day {
                let bar = usage::render_colored_progress_bar(d7.pct, 20);
                let rst = usage::format_countdown_cli(d7.countdown.as_deref());
                println!("  7-Day Limit:   {bar}{rst}");
            }
            if let Some(sp) = &u.spend {
                let bar = usage::render_colored_progress_bar(sp.pct, 20);
                let detail = if usage::is_no_color() {
                    format!("(${:.2} used of ${:.2} limit)", sp.used, sp.limit)
                } else {
                    format!("\x1b[38;2;148;163;184m(${:.2} used of ${:.2} limit)\x1b[0m", sp.used, sp.limit)
                };
                println!("  Extra Spend:   {bar} {detail}");
            }
            if !u.models.is_empty() {
                println!("\nPer-Model Weekly Limits:");
                for m in &u.models {
                    let bar = usage::render_colored_progress_bar(m.pct, 20);
                    let rst = usage::format_countdown_cli(m.countdown.as_deref());
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

    let raw_profile = &args[0];
    let profile = aliases::resolve_profile_name(raw_profile)?;
    let dir_arg = args.get(1).map(|s| s.as_str());
    let path = mappings::add_mapping(&profile, dir_arg)?;
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
    let raw = name.ok_or_else(|| anyhow!("usage: cuser disable <profile>"))?;
    let name = aliases::resolve_profile_name(&raw)?;
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
    let raw = name.ok_or_else(|| anyhow!("usage: cuser enable <profile>"))?;
    let name = aliases::resolve_profile_name(&raw)?;
    profiles::set_profile_disabled(&name, false)?;
    println!("Profile \"{name}\" is now enabled.");
    Ok(())
}

fn cmd_list(is_json: bool, force_refresh: bool, token_status: bool) -> Result<()> {
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
            let profile_aliases = aliases::aliases_for_profile(name).unwrap_or_default();
            let ts = if token_status {
                oauth::get_token_status(name)
            } else {
                None
            };
            list.push(ProfileListEntry {
                name: name.clone(),
                aliases: profile_aliases,
                is_current: current.as_deref() == Some(name),
                is_disabled: info.is_disabled,
                email: info.email,
                org_name: info.org_name,
                mapped_paths,
                usage: u,
                token_status: ts,
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
            let profile_aliases = aliases::aliases_for_profile(&name).unwrap_or_default();
            let alias_badge = if !profile_aliases.is_empty() {
                format!(" [alias: {}]", profile_aliases.join(", "))
            } else {
                String::new()
            };
            match (info.email, info.org_name) {
                (Some(email), Some(org)) => {
                    println!("{name}{alias_badge}  ({email} • {org}){disabled_badge}{badge}")
                }
                (Some(email), None) => println!("{name}{alias_badge}  ({email}){disabled_badge}{badge}"),
                (None, _) => println!("{name}{alias_badge}{disabled_badge}{badge}"),
            }

            if let Ok(u) = usage::get_profile_usage(&name, force_refresh) {
                match u.status {
                    usage::UsageStatus::Ok => {
                        if let Some(h5) = &u.five_hour {
                            let bar = usage::render_colored_progress_bar(h5.pct, 16);
                            let rst = usage::format_countdown_cli(h5.countdown.as_deref());
                            println!("    5h quota:  {bar}{rst}");
                        }
                        if let Some(d7) = &u.seven_day {
                            let bar = usage::render_colored_progress_bar(d7.pct, 16);
                            let rst = usage::format_countdown_cli(d7.countdown.as_deref());
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

            if token_status
                && let Some(ts) = oauth::get_token_status(&name)
            {
                println!("    token:     {} [{}]", ts.source, ts.token_type);
                if let Some(exp) = ts.access_token_expires_at {
                    let rst = usage::format_reset_countdown(&exp).map(|c| format!(" (expires in {c})")).unwrap_or_default();
                    let exp_note = if ts.access_token_expired { " [expired]" } else { "" };
                    println!("    access exp: {exp}{rst}{exp_note}");
                }
                if ts.refresh_token_present {
                    let r_exp = ts.refresh_token_expires_at.as_deref().unwrap_or("none");
                    println!("    refresh:   present (expires: {r_exp})");
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
                    let bar = usage::render_colored_progress_bar(h5.pct, 16);
                    let rst = usage::format_countdown_cli(h5.countdown.as_deref());
                    println!("  5h quota:  {bar}{rst}");
                }
                if let Some(d7) = &u.seven_day {
                    let bar = usage::render_colored_progress_bar(d7.pct, 16);
                    let rst = usage::format_countdown_cli(d7.countdown.as_deref());
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

fn cmd_add_token(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("usage: cuser add-token <token-or--> <profile> [--email <email>] [--org <org>]");
    }

    let raw_token = &args[0];
    let mut profile_name = None;
    let mut email = None;
    let mut org = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--email" => {
                if i + 1 < args.len() {
                    email = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    bail!("--email requires an argument");
                }
            }
            "--org" => {
                if i + 1 < args.len() {
                    org = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    bail!("--org requires an argument");
                }
            }
            arg if arg.starts_with("--email=") => {
                email = Some(arg.trim_start_matches("--email=").to_string());
                i += 1;
            }
            arg if arg.starts_with("--org=") => {
                org = Some(arg.trim_start_matches("--org=").to_string());
                i += 1;
            }
            val if !val.starts_with('-') && profile_name.is_none() => {
                profile_name = Some(val.to_string());
                i += 1;
            }
            other => {
                bail!("unknown argument: {other}");
            }
        }
    }

    let profile = match profile_name {
        Some(p) => p,
        None => prompt("Profile name for this token (e.g. \"backup\" or \"api-key\"): ")?.trim().to_string(),
    };

    token::add_token(raw_token, &profile, email.as_deref(), org.as_deref())
}

fn cmd_export(args: &[String]) -> Result<()> {
    let mut dest = None;
    let mut profile = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--profile" => {
                if i + 1 < args.len() {
                    profile = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    bail!("--profile requires an argument");
                }
            }
            arg if arg.starts_with("--profile=") => {
                profile = Some(arg.trim_start_matches("--profile=").to_string());
                i += 1;
            }
            val if (val == "-" || !val.starts_with('-')) && dest.is_none() => {
                dest = Some(val.to_string());
                i += 1;
            }
            other => {
                bail!("unknown argument for export: {other}");
            }
        }
    }

    transfer::export_accounts(dest.as_deref(), profile.as_deref())
}

fn cmd_import(args: &[String]) -> Result<()> {
    let force = args.iter().any(|a| a == "--force");
    let filtered: Vec<&str> = args
        .iter()
        .filter(|a| *a != "--force")
        .map(|s| s.as_str())
        .collect();

    let first = filtered.first().copied();

    // Check if the argument is a backup file (file exists or ends in .json / .cuser)
    if let Some(target) = first {
        let p = std::path::Path::new(target);
        if p.exists() || target.ends_with(".json") || target.ends_with(".cuser") {
            return transfer::import_accounts(p, force);
        }
    }

    // Otherwise import ~/.claude directory
    let name = match first {
        Some(n) => n.to_string(),
        None => prompt("Name for this account (e.g. \"main\"): ")?.trim().to_string(),
    };
    profiles::import_default(&name)?;
    println!("Imported ~/.claude as profile \"{name}\".");
    Ok(())
}

fn cmd_alias(args: &[String], is_json: bool) -> Result<()> {
    if args.is_empty() {
        let all = aliases::load_aliases()?;
        if is_json {
            println!("{}", serde_json::to_string_pretty(&all)?);
            return Ok(());
        }
        if all.is_empty() {
            println!("No aliases configured yet. Run `cuser alias <profile> <alias>` to create one.");
            return Ok(());
        }
        println!("Configured Aliases:");
        for (alias, profile) in all {
            println!("  {alias} -> {profile}");
        }
        return Ok(());
    }

    if args.len() == 2 && args[1] == "--unset" {
        let alias = &args[0];
        let removed = aliases::remove_alias(alias)?;
        match removed {
            Some(p) => println!("Removed alias \"{alias}\" (was aliased to \"{p}\")."),
            None => println!("No alias found named \"{alias}\"."),
        }
        return Ok(());
    }

    if args.len() < 2 {
        bail!("usage: cuser alias <profile> <alias> (or `cuser alias <alias> --unset`)");
    }

    let profile = &args[0];
    let alias = &args[1];
    aliases::set_alias(profile, alias)
}

fn cmd_unalias(alias: Option<String>) -> Result<()> {
    let alias = alias.ok_or_else(|| anyhow!("usage: cuser unalias <alias>"))?;
    let removed = aliases::remove_alias(&alias)?;
    match removed {
        Some(p) => println!("Removed alias \"{alias}\" (was aliased to \"{p}\")."),
        None => println!("No alias found named \"{alias}\"."),
    }
    Ok(())
}

fn cmd_config(args: &[String], is_json: bool) -> Result<()> {
    if args.is_empty() || args[0] == "list" {
        let s = settings::ToolSettings::load()?;
        if is_json {
            println!("{}", serde_json::to_string_pretty(&s)?);
            return Ok(());
        }
        println!("Configuration (from {}):\n", settings::settings_file()?.display());
        let thresh_note = if s.autoswitch.as_ref().and_then(|a| a.threshold).is_some() { "" } else { " (default)" };
        println!("  autoswitch.threshold = {:.1}%{thresh_note}", s.effective_threshold());

        let strat_note = if s.autoswitch.as_ref().and_then(|a| a.strategy.as_ref()).is_some() { "" } else { " (default)" };
        println!("  autoswitch.strategy  = {}{strat_note}", s.effective_strategy());

        let model_str = s.effective_model().unwrap_or_else(|| "(none)".to_string());
        println!("  autoswitch.model     = {model_str}");

        let int_note = if s.autoswitch.as_ref().and_then(|a| a.interval).is_some() { "" } else { " (default)" };
        println!("  autoswitch.interval  = {}s{int_note}", s.effective_interval());

        return Ok(());
    }

    match args[0].as_str() {
        "path" => {
            println!("{}", settings::settings_file()?.display());
            Ok(())
        }
        "get" => {
            let key = args.get(1).ok_or_else(|| anyhow!("usage: cuser config get <key>"))?;
            let val = settings::get_setting(key)?;
            match val {
                Some(v) => println!("{v}"),
                None => println!("(default)"),
            }
            Ok(())
        }
        "set" => {
            let key = args.get(1).ok_or_else(|| anyhow!("usage: cuser config set <key> <value>"))?;
            let val = args.get(2).ok_or_else(|| anyhow!("usage: cuser config set <key> <value>"))?;
            settings::set_setting(key, val)
        }
        "unset" => {
            let key = args.get(1).ok_or_else(|| anyhow!("usage: cuser config unset <key>"))?;
            settings::unset_setting(key)
        }
        other => {
            bail!("unknown config action: '{other}'. Usage: `cuser config [get|set|unset|path]`");
        }
    }
}

fn cmd_import_usage(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("usage: cuser import-usage <file|-> [--hold <seconds>]");
    }

    let src = &args[0];
    let hold_secs = args
        .iter()
        .position(|a| a == "--hold")
        .and_then(|pos| args.get(pos + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600);

    let content = if src == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(src)
            .with_context(|| format!("failed to read usage file: {src}"))?
    };

    let count = usage::import_usage_data(&content, hold_secs)?;
    println!("Imported usage readings for {count} profile(s).");
    Ok(())
}

fn cmd_remove(name: Option<String>) -> Result<()> {
    let raw = name.ok_or_else(|| anyhow!("usage: cuser remove <profile>"))?;
    let name = aliases::resolve_profile_name(&raw)?;
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
    let old_raw = old.ok_or_else(|| anyhow!("usage: cuser rename <old> <new>"))?;
    let old = aliases::resolve_profile_name(&old_raw)?;
    let new = new.ok_or_else(|| anyhow!("usage: cuser rename <old> <new>"))?;
    profiles::rename_profile(&old, &new)?;
    println!("Renamed \"{old}\" to \"{new}\".");
    Ok(())
}

fn cmd_purge() -> Result<()> {
    let root = profiles::profiles_root()?;
    let answer = prompt(&format!(
        "This will permanently delete all claude-user data in {} (including stored profiles and logins).\nContinue? [y/N] ",
        root.display()
    ))?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        println!("Cancelled.");
        return Ok(());
    }
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    println!("Purged {}. All claude-user data has been removed.", root.display());
    Ok(())
}

fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input)
}
