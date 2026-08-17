use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub fn launch_claude(profile_dir: &Path, args: &[String]) -> Result<()> {
    let mut cmd = Command::new("claude");
    cmd.env("CLAUDE_CONFIG_DIR", profile_dir);
    cmd.args(args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
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
