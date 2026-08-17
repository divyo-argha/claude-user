pub mod launch;
pub mod profiles;
pub mod tui;

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
    cuser import [name]      import your currently logged-in ~/.claude as a new profile
    cuser --help | -h        show this help

The picker (plain `cuser`) also offers \"+ Import ~/.claude\" whenever a
default ~/.claude exists, and \"+ New profile\" to log into a brand-new account.
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
        "list" | "-l" => cmd_list(),
        "sync" => cmd_sync(),
        "import" | "migrate" => cmd_import(args.get(1).cloned()),
        name => {
            profiles::validate_profile_name(name)?;
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

fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input)
}
