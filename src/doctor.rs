use anyhow::Result;
use std::fs;
use std::process::Command;

use crate::aliases;
use crate::mappings;
use crate::oauth;
use crate::profiles;
use crate::usage;

pub struct DoctorReport {
    pub claude_installed: bool,
    pub claude_version: Option<String>,
    pub symlink_claude_ok: bool,
    pub symlink_claude_msg: String,
    pub symlink_json_ok: bool,
    pub symlink_json_msg: String,
    pub profiles_checked: usize,
    pub profiles_issues: Vec<String>,
    pub orphaned_mappings: Vec<String>,
    pub broken_aliases: Vec<String>,
    pub permission_issues: Vec<String>,
}

pub fn run_doctor(auto_fix: bool) -> Result<()> {
    println!("Running Claude User Diagnostic Doctor...\n");
    let mut report = check_system()?;

    print_report(&report);

    if auto_fix {
        println!("\nApplying automatic repairs (--fix)...");
        let fixes = apply_fixes(&mut report)?;
        if fixes.is_empty() {
            println!("  No repairable issues found.");
        } else {
            for f in fixes {
                println!("  ✔ {f}");
            }
            println!("\nRepairs complete! Re-checking system:");
            let after = check_system()?;
            print_report(&after);
        }
    } else {
        let has_issues = !report.symlink_claude_ok
            || !report.symlink_json_ok
            || !report.orphaned_mappings.is_empty()
            || !report.broken_aliases.is_empty()
            || !report.permission_issues.is_empty();

        if has_issues {
            println!("\nTip: Run `cuser doctor --fix` to automatically repair symlinks, broken aliases, orphaned mappings, and permissions.");
        }
    }

    Ok(())
}

fn check_system() -> Result<DoctorReport> {
    // 1. Claude CLI check
    let (claude_installed, claude_version) = match Command::new("claude").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (true, Some(ver))
        }
        _ => (false, None),
    };

    // 2. Symlinks check
    let claude_dir = profiles::default_claude_dir()?;
    let (symlink_claude_ok, symlink_claude_msg) = if !claude_dir.exists() {
        (false, "Directory ~/.claude does not exist".to_string())
    } else if !profiles::is_symlink(&claude_dir) {
        (
            false,
            "~/.claude is a physical folder, not a symlink (run `cuser import <name>` to migrate)"
                .to_string(),
        )
    } else {
        match fs::read_link(&claude_dir) {
            Ok(target) => {
                if target.exists() {
                    (true, format!("Points to {}", target.display()))
                } else {
                    (false, format!("Broken symlink -> {}", target.display()))
                }
            }
            Err(e) => (false, format!("Failed to read symlink: {e}")),
        }
    };

    let claude_json = profiles::default_claude_json()?;
    let (symlink_json_ok, symlink_json_msg) = if !claude_json.exists() {
        (false, "~/.claude.json does not exist".to_string())
    } else if !profiles::is_symlink(&claude_json) {
        (
            false,
            "~/.claude.json is a physical file, not a symlink".to_string(),
        )
    } else {
        match fs::read_link(&claude_json) {
            Ok(target) => {
                if target.exists() {
                    (true, format!("Points to {}", target.display()))
                } else {
                    (false, format!("Broken symlink -> {}", target.display()))
                }
            }
            Err(e) => (false, format!("Failed to read symlink: {e}")),
        }
    };

    // 3. Profiles check
    let all_profiles = profiles::list_profiles()?;
    let mut profiles_issues = Vec::new();
    let mut permission_issues = Vec::new();

    for name in &all_profiles {
        let dir = profiles::profile_dir(name)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&dir) {
                let mode = meta.permissions().mode() & 0o777;
                if mode != 0o700 {
                    permission_issues.push(format!(
                        "Profile \"{name}\" directory has permissions {mode:o} (expected 700)"
                    ));
                }
            }
        }

        // Credentials check
        if let Some(token_summary) = oauth::get_token_status(name)
            && token_summary.access_token_expired
        {
            profiles_issues.push(format!(
                "Profile \"{name}\": Access token expired (run `cuser {name}` to renew)"
            ));
        }

        // Check usage endpoint cached status
        if let Ok(u) = usage::get_profile_usage(name, false)
            && u.status == usage::UsageStatus::RateLimited
        {
            profiles_issues.push(format!("Profile \"{name}\": Rate-limited on Anthropic usage API (429)"));
        }
    }

    // 4. Mappings check
    let mut orphaned_mappings = Vec::new();
    if let Ok(mappings) = mappings::load_mappings() {
        for (path, profile) in mappings {
            if !path.exists() {
                orphaned_mappings.push(format!(
                    "Path \"{}\" mapped to \"{profile}\" does not exist",
                    path.display()
                ));
            } else if !profiles::profile_exists(&profile)? {
                orphaned_mappings.push(format!(
                    "Path \"{}\" mapped to nonexistent profile \"{profile}\"",
                    path.display()
                ));
            }
        }
    }

    // 5. Aliases check
    let mut broken_aliases = Vec::new();
    if let Ok(alias_map) = aliases::load_aliases() {
        for (alias, target) in alias_map {
            if !profiles::profile_exists(&target)? {
                broken_aliases.push(format!(
                    "Alias \"{alias}\" points to nonexistent profile \"{target}\""
                ));
            }
        }
    }

    Ok(DoctorReport {
        claude_installed,
        claude_version,
        symlink_claude_ok,
        symlink_claude_msg,
        symlink_json_ok,
        symlink_json_msg,
        profiles_checked: all_profiles.len(),
        profiles_issues,
        orphaned_mappings,
        broken_aliases,
        permission_issues,
    })
}

fn print_report(r: &DoctorReport) {
    // 1. Claude CLI
    if r.claude_installed {
        let ver = r.claude_version.as_deref().unwrap_or("unknown");
        println!("  [✔] Claude CLI installed ({ver})");
    } else {
        println!("  [✖] Claude CLI not found on PATH! (install via: npm install -g @anthropic-ai/claude-code)");
    }

    // 2. Symlinks
    if r.symlink_claude_ok {
        println!("  [✔] ~/.claude symlink: {}", r.symlink_claude_msg);
    } else {
        println!("  [✖] ~/.claude symlink: {}", r.symlink_claude_msg);
    }

    if r.symlink_json_ok {
        println!("  [✔] ~/.claude.json symlink: {}", r.symlink_json_msg);
    } else {
        println!("  [✖] ~/.claude.json symlink: {}", r.symlink_json_msg);
    }

    // 3. Profiles
    println!("  [✔] Checked {} profile(s)", r.profiles_checked);
    for issue in &r.profiles_issues {
        println!("      ⚠ {issue}");
    }

    // 4. Permissions
    if r.permission_issues.is_empty() {
        println!("  [✔] Profile directories filesystem permissions: Hardened (0700)");
    } else {
        for p in &r.permission_issues {
            println!("  [✖] {p}");
        }
    }

    // 5. Mappings
    if r.orphaned_mappings.is_empty() {
        println!("  [✔] Directory mappings: All targets valid");
    } else {
        for m in &r.orphaned_mappings {
            println!("  [✖] Orphaned directory mapping: {m}");
        }
    }

    // 6. Aliases
    if r.broken_aliases.is_empty() {
        println!("  [✔] Profile aliases: All targets valid");
    } else {
        for a in &r.broken_aliases {
            println!("  [✖] Broken alias: {a}");
        }
    }
}

fn apply_fixes(_report: &mut DoctorReport) -> Result<Vec<String>> {
    let mut fixes = Vec::new();

    // 1. Repair symlinks if broken
    let current_opt = profiles::current_profile()?;
    let all_profs = profiles::list_profiles()?;
    let repair_target = current_opt.as_deref().or_else(|| all_profs.first().map(|s| s.as_str()));

    if let Some(target_profile) = repair_target {
        let claude_dir = profiles::default_claude_dir()?;
        let claude_json = profiles::default_claude_json()?;
        let need_dir_repair = !claude_dir.exists() || !profiles::is_symlink(&claude_dir);
        let need_json_repair = !claude_json.exists() || !profiles::is_symlink(&claude_json);

        if need_dir_repair || need_json_repair {
            profiles::activate_profile(target_profile)?;
            fixes.push(format!("Re-linked ~/.claude and ~/.claude.json to \"{target_profile}\""));
        }
    }

    // 2. Prune orphaned directory mappings
    let mut mappings_map = mappings::load_mappings()?;
    let before_count = mappings_map.len();
    mappings_map.retain(|path, prof| path.exists() && profiles::profile_exists(prof).unwrap_or(false));
    if mappings_map.len() != before_count {
        mappings::save_mappings(&mappings_map)?;
        let removed = before_count - mappings_map.len();
        fixes.push(format!("Pruned {removed} orphaned directory mapping(s)"));
    }

    // 3. Prune broken aliases
    let mut aliases_map = aliases::load_aliases()?;
    let before_alias_count = aliases_map.len();
    aliases_map.retain(|_, target| profiles::profile_exists(target).unwrap_or(false));
    if aliases_map.len() != before_alias_count {
        aliases::save_aliases(&aliases_map)?;
        let removed = before_alias_count - aliases_map.len();
        fixes.push(format!("Pruned {removed} broken alias(es)"));
    }

    // 4. Harden permissions on profiles
    for name in &all_profs {
        let dir = profiles::profile_dir(name)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
            let creds = dir.join(".credentials.json");
            if creds.exists() {
                let _ = fs::set_permissions(&creds, fs::Permissions::from_mode(0o600));
            }
            let cjson = dir.join(".claude.json");
            if cjson.exists() {
                let _ = fs::set_permissions(&cjson, fs::Permissions::from_mode(0o600));
            }
        }
    }
    fixes.push("Hardened directory and credential permissions (0700 / 0600)".to_string());

    Ok(fixes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_system_executes() {
        let report = check_system();
        assert!(report.is_ok());
    }
}
