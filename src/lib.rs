pub mod launch;
pub mod profiles;
pub mod tui;
pub mod update;

use anyhow::{anyhow, bail, Result};
use std::io::{self, Write};

const HELP: &str = "\
claude-user — switch between Claude accounts (alias: cuser)

USAGE:
    claude-user                    open the interactive profile picker
    claude-user <profile>          launch that profile directly (created if new)
    claude-user <profile> [args]   launch that profile, passing [args] to `claude`
    claude-user list | -l          list existing profiles
    claude-user sync               copy shared config into every existing profile
    claude-user import [name]      import your currently logged-in ~/.claude as a new profile
    claude-user remove <profile>   delete a profile (asks for confirmation)
    claude-user rename <old> <new> rename a profile
    claude-user --update           update claude-user to the latest release
    claude-user --version | -v     show the installed version
    claude-user --help | -h        show this help

The picker (plain `claude-user` / `cuser`) also offers \"+ Import ~/.claude\" whenever a
default ~/.claude exists, and \"+ New profile\" to log into a brand-new account.
Highlighting an existing profile in the picker also offers `d` to delete it
and `r` to rename it.

Launching a profile also points ~/.claude and ~/.claude.json at it (via
symlinks), so `claude` run directly afterward uses that same account. If
~/.claude already exists as a real directory, run `claude-user import <name>` first.
";

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
        "list" | "-l" => cmd_list(),
        "sync" => cmd_sync(),
        "import" | "migrate" => cmd_import(args.get(1).cloned()),
        "remove" | "rm" | "delete" => cmd_remove(args.get(1).cloned()),
        "rename" => cmd_rename(args.get(1).cloned(), args.get(2).cloned()),
        "--update" | "update" => update::run(),
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
    profiles::activate_profile(name)?;
    let dir = profiles::profile_dir(name)?;
    launch::launch_claude(&dir, args)
}

fn cmd_list() -> Result<()> {
    let names = profiles::list_profiles()?;
    if names.is_empty() {
        println!("No profiles yet. Run `cuser <name>` to create one.");
    } else {
        for name in names {
            let info = profiles::get_profile_info(&name)?;
            match (info.email, info.org_name) {
                (Some(email), Some(org)) => println!("{name}  ({email} • {org})"),
                (Some(email), None) => println!("{name}  ({email})"),
                (None, _) => println!("{name}"),
            }
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
