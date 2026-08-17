use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

const REPO: &str = "divyo-argha/claude-user";

#[derive(Deserialize)]
struct ReleaseInfo {
    tag_name: String,
}

pub fn run() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("Checking for updates...");

    let latest_tag = fetch_latest_tag()?;
    let latest = latest_tag.trim_start_matches('v').to_string();

    if !is_newer(&latest, current) {
        println!("Already up to date (v{current}).");
        return Ok(());
    }

    let current_exe =
        env::current_exe().context("could not determine the running executable's path")?;
    if is_npm_managed(&current_exe) {
        bail!(
            "This install of claude-user is managed by npm.\n\
             Update it with: npm install -g claude-user@latest"
        );
    }

    let target = target_triple().ok_or_else(|| {
        anyhow::anyhow!(
            "claude-user does not ship a prebuilt binary for {}/{}.\n\
             Update manually: https://github.com/{REPO}#-install",
            env::consts::OS,
            env::consts::ARCH
        )
    })?;

    println!(
        "Updating claude-user {current} → {latest} ({} {})",
        env::consts::OS,
        env::consts::ARCH
    );

    let url =
        format!("https://github.com/{REPO}/releases/download/{latest_tag}/claude-user-{target}.tar.gz");
    let bytes = download(&url)?;

    let tmp_dir = env::temp_dir().join(format!("claude-user-update-{}", std::process::id()));
    let result = perform_update(&bytes, &tmp_dir, &latest, &current_exe);
    let _ = fs::remove_dir_all(&tmp_dir);
    result?;

    println!("Updated to claude-user {latest}. Downloaded, verified, installed.");
    Ok(())
}

fn perform_update(bytes: &[u8], tmp_dir: &Path, latest: &str, current_exe: &Path) -> Result<()> {
    extract_tarball(bytes, tmp_dir)?;
    verify_binary(&tmp_dir.join("cuser"), latest)?;
    install_binary(&tmp_dir.join("claude-user"), &current_exe.with_file_name("claude-user"))?;
    install_binary(&tmp_dir.join("cuser"), &current_exe.with_file_name("cuser"))?;
    Ok(())
}

fn fetch_latest_tag() -> Result<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut response = ureq::get(&url)
        .header("User-Agent", "claude-user-updater")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => {
                anyhow::anyhow!("no releases have been published yet for {REPO}")
            }
            other => anyhow::Error::new(other).context("failed to check for updates"),
        })?;
    let body = response
        .body_mut()
        .read_to_string()
        .context("failed to read release info")?;
    let info: ReleaseInfo =
        serde_json::from_str(&body).context("failed to parse release info")?;
    Ok(info.tag_name)
}

fn download(url: &str) -> Result<Vec<u8>> {
    let mut response = ureq::get(url)
        .header("User-Agent", "claude-user-updater")
        .call()
        .context("failed to download update")?;
    response
        .body_mut()
        .with_config()
        .limit(200 * 1024 * 1024)
        .read_to_vec()
        .context("failed to read downloaded update")
}

fn extract_tarball(bytes: &[u8], dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    let tarball_path = dest.join("update.tar.gz");
    fs::write(&tarball_path, bytes)?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&tarball_path)
        .arg("-C")
        .arg(dest)
        .status()
        .context("failed to run `tar` to extract the update (is it installed?)")?;
    if !status.success() {
        bail!("`tar` failed to extract the downloaded update");
    }
    Ok(())
}

fn verify_binary(path: &Path, expected_version: &str) -> Result<()> {
    set_executable(path)?;
    let output = Command::new(path)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to run downloaded binary at {}", path.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !stdout.contains(expected_version) {
        bail!(
            "downloaded binary failed verification (expected version {expected_version}, got: {})",
            stdout.trim()
        );
    }
    Ok(())
}

fn install_binary(src: &Path, dst: &Path) -> Result<()> {
    let bytes = fs::read(src)?;
    let parent = dst
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid install path: {}", dst.display()))?;
    let file_name = dst.file_name().unwrap().to_string_lossy();
    let tmp = parent.join(format!(".{file_name}.update"));
    fs::write(&tmp, &bytes).map_err(|e| classify_permission_error(e, parent))?;
    set_executable(&tmp)?;
    fs::rename(&tmp, dst)?;
    Ok(())
}

fn classify_permission_error(e: io::Error, dir: &Path) -> anyhow::Error {
    if e.kind() == io::ErrorKind::PermissionDenied {
        anyhow::anyhow!(
            "permission denied writing to {}\nRetry with: sudo claude-user --update",
            dir.display()
        )
    } else {
        e.into()
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn is_npm_managed(exe: &Path) -> bool {
    exe.components().any(|c| c.as_os_str() == "node_modules")
}

fn target_triple() -> Option<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

fn parse_version(v: &str) -> (u64, u64, u64) {
    let mut parts = v.split('.').map(|p| {
        p.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn is_newer(latest: &str, current: &str) -> bool {
    parse_version(latest) > parse_version(current)
}
