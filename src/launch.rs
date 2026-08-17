use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Launch `claude` with CLAUDE_CONFIG_DIR pointed at the given profile directory,
/// forwarding any extra args. On Unix this replaces the current process image;
/// on Windows it spawns a child and propagates its exit code.
pub fn launch_claude(profile_dir: &Path, args: &[String]) -> Result<()> {
    let mut cmd = Command::new("claude");
    cmd.env("CLAUDE_CONFIG_DIR", profile_dir);
    cmd.args(args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        // exec() only returns on failure.
        Err(err).context("failed to launch `claude` (is it installed and on PATH?)")
    }

    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .context("failed to launch `claude` (is it installed and on PATH?)")?;
        std::process::exit(status.code().unwrap_or(1));
    }
}
