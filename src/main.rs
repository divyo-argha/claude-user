mod launch;
mod profiles;
mod tui;

use anyhow::Result;
use std::io::{self, Write};

const HELP: &str = "\
cuser / claude-user — switch between Claude accounts

USAGE:
    cuser                    open the interactive profile picker
    cuser <profile>          launch that profile directly (created if new)
    cuser <profile> [args]   launch that profile, passing [args] to `claude`
    cuser list | -l          list existing profiles
    cuser sync               copy shared config into every existing profile
    cuser migrate [name]     import your existing ~/.claude as a named profile
    cuser --help | -h        show this help
";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        return run_default();
    }

    match args[0].as_str() {
        "--help" | "-h" | "help" => {
            print!("{HELP}");
            Ok(())
        }
        "list" | "-l" => cmd_list(),
        "sync" => cmd_sync(),
        "migrate" | "import" => cmd_migrate(args.get(1).cloned()),
        name => {
            profiles::validate_profile_name(name)?;
            let created = profiles::ensure_profile(name)?;
            if created {
                eprintln!("Creating new profile: {name}");
            }
            let dir = profiles::profile_dir(name)?;
            launch::launch_claude(&dir, &args[1..])
        }
    }
}

fn run_default() -> Result<()> {
    if profiles::needs_migration()? {
        eprintln!("No Claude profiles set up yet, but an existing ~/.claude was found.");
        let name = prompt("Name for this account (e.g. \"main\"): ")?;
        profiles::migrate_existing(name.trim())?;
        println!("Imported ~/.claude as profile \"{}\".", name.trim());
        let dir = profiles::profile_dir(name.trim())?;
        return launch::launch_claude(&dir, &[]);
    }

    match tui::run_picker()? {
        Some(name) => {
            profiles::validate_profile_name(&name)?;
            let created = profiles::ensure_profile(&name)?;
            if created {
                eprintln!("Creating new profile: {name}");
            }
            let dir = profiles::profile_dir(&name)?;
            launch::launch_claude(&dir, &[])
        }
        None => Ok(()),
    }
}

fn cmd_list() -> Result<()> {
    let names = profiles::list_profiles()?;
    if names.is_empty() {
        println!("No profiles yet. Run `cuser <name>` to create one.");
    } else {
        for name in names {
            println!("{name}");
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

fn cmd_migrate(name: Option<String>) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => prompt("Name for this account (e.g. \"main\"): ")?.trim().to_string(),
    };
    profiles::migrate_existing(&name)?;
    println!("Imported ~/.claude as profile \"{name}\".");
    Ok(())
}

fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input)
}
