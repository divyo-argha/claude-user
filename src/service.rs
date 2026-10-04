use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub fn install_service() -> Result<()> {
    let exe = std::env::current_exe()
        .context("failed to determine current executable path")?
        .canonicalize()
        .context("failed to canonicalize executable path")?;
    let exe_str = exe.to_string_lossy();

    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().context("could not locate home directory")?;
        let launch_agents = home.join("Library/LaunchAgents");
        fs::create_dir_all(&launch_agents)?;

        let plist_path = launch_agents.join("com.claude-user.auto.plist");
        let log_dir = home.join("Library/Logs");
        fs::create_dir_all(&log_dir)?;

        let log_out = log_dir.join("claude-user.log").to_string_lossy().into_owned();
        let log_err = log_dir.join("claude-user.err").to_string_lossy().into_owned();

        let plist_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.claude-user.auto</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_str}</string>
        <string>auto</string>
        <string>--notify</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log_out}</string>
    <key>StandardErrorPath</key>
    <string>{log_err}</string>
</dict>
</plist>
"#
        );

        fs::write(&plist_path, plist_content)?;
        let _ = Command::new("launchctl")
            .arg("unload")
            .arg(&plist_path)
            .status();
        let status = Command::new("launchctl")
            .arg("load")
            .arg("-w")
            .arg(&plist_path)
            .status()
            .context("failed to execute launchctl load")?;

        if !status.success() {
            bail!("launchctl load failed with exit code: {:?}", status.code());
        }

        println!("Successfully installed and started background service via launchd!");
        println!("  Service:  com.claude-user.auto");
        println!("  Plist:    {}", plist_path.display());
        println!("  Logs:     {log_out}");
        println!("Claude User will now monitor rate limits in the background at all times.");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().context("could not locate home directory")?;
        let systemd_dir = home.join(".config/systemd/user");
        fs::create_dir_all(&systemd_dir)?;

        let service_path = systemd_dir.join("claude-user-auto.service");
        let service_content = format!(
            r#"[Unit]
Description=Claude User Auto-Switch Daemon
After=network.target

[Service]
ExecStart={exe_str} auto --notify
Restart=always
RestartSec=10

[Install]
WantedBy=default.target
"#
        );

        fs::write(&service_path, service_content)?;
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        let status = Command::new("systemctl")
            .args(["--user", "enable", "--now", "claude-user-auto.service"])
            .status()
            .context("failed to enable systemd service")?;

        if !status.success() {
            bail!("systemctl failed with exit code: {:?}", status.code());
        }

        println!("Successfully installed and started background service via systemd!");
        println!("  Service:  claude-user-auto.service");
        println!("  Unit:     {}", service_path.display());
        println!("Claude User will now monitor rate limits in the background at all times.");
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        bail!("background service installation is currently supported on macOS and Linux");
    }
}

pub fn uninstall_service() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().context("could not locate home directory")?;
        let plist_path = home.join("Library/LaunchAgents/com.claude-user.auto.plist");
        if plist_path.exists() {
            let _ = Command::new("launchctl")
                .arg("unload")
                .arg(&plist_path)
                .status();
            fs::remove_file(&plist_path)?;
            println!("Uninstalled and stopped launchd service (com.claude-user.auto).");
        } else {
            println!("No installed launchd service found at {}", plist_path.display());
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().context("could not locate home directory")?;
        let service_path = home.join(".config/systemd/user/claude-user-auto.service");
        if service_path.exists() {
            let _ = Command::new("systemctl")
                .args(["--user", "disable", "--now", "claude-user-auto.service"])
                .status();
            fs::remove_file(&service_path)?;
            let _ = Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status();
            println!("Uninstalled and stopped systemd service (claude-user-auto.service).");
        } else {
            println!("No installed systemd service found at {}", service_path.display());
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        bail!("background service management is supported on macOS and Linux");
    }
}

pub fn service_status() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().context("could not locate home directory")?;
        let plist_path = home.join("Library/LaunchAgents/com.claude-user.auto.plist");
        let installed = plist_path.exists();

        println!("Claude User Auto-Switch Service (macOS launchd):");
        println!("  Installed: {}", if installed { "Yes" } else { "No" });
        println!("  Plist:     {}", plist_path.display());

        if installed {
            let output = Command::new("launchctl")
                .args(["list", "com.claude-user.auto"])
                .output();
            if let Ok(out) = output {
                if out.status.success() {
                    let text = String::from_utf8_lossy(&out.stdout);
                    println!("  Status:    Running");
                    if let Some(pid_line) = text.lines().find(|l| l.contains("\"PID\"")) {
                        println!("  {}", pid_line.trim());
                    }
                } else {
                    println!("  Status:    Loaded but idle / stopped");
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().context("could not locate home directory")?;
        let service_path = home.join(".config/systemd/user/claude-user-auto.service");
        let installed = service_path.exists();

        println!("Claude User Auto-Switch Service (Linux systemd):");
        println!("  Installed: {}", if installed { "Yes" } else { "No" });
        println!("  Unit:      {}", service_path.display());

        if installed {
            let _ = Command::new("systemctl")
                .args(["--user", "status", "claude-user-auto.service"])
                .status();
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        bail!("background service management is supported on macOS and Linux");
    }
}

pub fn service_file_path() -> Result<Option<PathBuf>> {
    let home = dirs::home_dir().context("could not locate home directory")?;
    #[cfg(target_os = "macos")]
    {
        Ok(Some(home.join("Library/LaunchAgents/com.claude-user.auto.plist")))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Some(home.join(".config/systemd/user/claude-user-auto.service")))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(None)
    }
}
