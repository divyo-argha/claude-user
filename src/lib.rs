pub mod launch;
pub mod mappings;
pub mod profiles;
pub mod tui;
pub mod update;

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::io::{self, Write};
use std::path::PathBuf;

const HELP: &str = "\
claude-user — switch between Claude accounts (alias: cuser)

USAGE:
    claude-user                    open the interactive profile picker
    claude-user <profile>          launch that profile directly (created if new)
    claude-user <profile> [args]   launch that profile, passing [args] to `claude`
    claude-user run [args]         launch profile mapped to current directory
    claude-user list | -l [--json] list existing profiles
    claude-user current [--json]   show the profile currently pointed to by `claude`
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
}

pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        return run_picker();
    }

    match args[0].as_str() {
        "--help" | "-h" | "help" => {
            print!("{HELP}");
            Ok(())
        }
        "--version" | "-v" => {
            println!("claude-user {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "list" | "-l" => {
            let is_json = args.iter().any(|a| a == "--json");
            cmd_list(is_json)
        }
        "current" | "active" | "status" => {
            let is_json = args.iter().any(|a| a == "--json");
            cmd_current(is_json)
        }
        "run" => cmd_run(&args[1..]),
        "map" => {
            let is_json = args.iter().any(|a| a == "--json");
            let filtered: Vec<String> = args
                .iter()
                .skip(1)
                .filter(|a| *a != "--json")
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
        "--json" => cmd_list(true),
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

fn cmd_list(is_json: bool) -> Result<()> {
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
            list.push(ProfileListEntry {
                name: name.clone(),
                is_current: current.as_deref() == Some(name),
                is_disabled: info.is_disabled,
                email: info.email,
                org_name: info.org_name,
                mapped_paths,
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
        }
    }
    Ok(())
}

fn cmd_current(is_json: bool) -> Result<()> {
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
