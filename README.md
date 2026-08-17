<div align="center">
  <br />

  <pre style="line-height: 1.25; font-weight: bold; background: none; border: none; padding: 0; display: inline-block; text-align: left; font-family: monospace;">
  <span style="color: #D97757;">░█▀▀░█░░░█▀█░█░█░█▀▄░█▀▀</span><span style="color: #94A3B8;">░░░░░█░█░█▀▀░█▀▀░█▀▄</span>
  <span style="color: #D97757;">░█░░░█░░░█▀█░█░█░█░█░█▀▀</span><span style="color: #94A3B8;">░▄▄▄░█░█░▀▀█░█▀▀░█▀▄</span>
  <span style="color: #D97757;">░▀▀▀░▀▀▀░▀░▀░▀▀▀░▀▀░░▀▀▀</span><span style="color: #94A3B8;">░░░░░▀▀▀░▀▀▀░▀▀▀░▀░▀</span>
  </pre>

  <p>
    <strong>One command to switch Claude Code accounts.</strong><br />
    Stop logging out. Stop re-authenticating. Stop mixing up work and personal chats.
  </p>

  <p>
    <a href="https://github.com/divyo-argha/claude-user/releases"><img src="https://img.shields.io/github/v/release/divyo-argha/claude-user?style=flat&color=D97757&label=latest" alt="Latest Release" /></a>
    <a href="https://www.npmjs.com/package/claude-user"><img src="https://img.shields.io/npm/v/claude-user?style=flat&color=CB3837&logo=npm&logoColor=white&label=npm" alt="npm" /></a>
    <a href="https://www.npmjs.com/package/claude-user"><img src="https://img.shields.io/npm/dt/claude-user?style=flat&color=CB3837&logo=npm&logoColor=white&label=npm%20downloads" alt="npm downloads" /></a>
    <a href="Cargo.toml"><img src="https://img.shields.io/badge/Rust-1.85+-000000?style=flat&logo=rust&logoColor=white" alt="Rust" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-22c55e?style=flat" alt="MIT" /></a>
  </p>

  <p>
    <a href="#-the-problem">The Problem</a> ·
    <a href="#-install">Install</a> ·
    <a href="#-quick-start">Quick Start</a> ·
    <a href="#-why-claude-user">Why claude-user</a> ·
    <a href="#-features">Features</a> ·
    <a href="#-interactive-tui">TUI</a> ·
    <a href="#-commands">Commands</a> ·
    <a href="#-security">Security</a> ·
    <a href="#-troubleshooting">Troubleshooting</a> ·
    <a href="#-contributing">Contributing</a>
  </p>

  <br />

  <img src="https://img.shields.io/badge/Linux-supported-FCC624?style=flat&logo=linux&logoColor=black" alt="Linux" />
  <img src="https://img.shields.io/badge/macOS-supported-000000?style=flat&logo=apple&logoColor=white" alt="macOS" />
  <img src="https://img.shields.io/badge/Windows-build%20from%20source-lightgrey?style=flat&logo=windows&logoColor=white" alt="Windows" />

  <br /><br />
</div>

---

## 📑 Table of Contents

**Getting Started**
- [🎯 The Problem](#-the-problem)
- [📦 Install](#-install)
- [⬆️ Updating](#-updating)
- [🗑️ Uninstall](#-uninstall)
- [⚡ Quick Start](#-quick-start)

**Accounts & Profiles**
- [🏆 Why claude-user?](#-why-claude-user)
- [✨ Features](#-features)
- [🖥️ Interactive TUI](#-interactive-tui)
- [🔄 How It Works](#-how-it-works)

**Security**
- [🛡️ Security](#-security)

**Reference**
- [📋 Commands](#-commands)
- [📁 Directory Layout](#-directory-layout)
- [🔧 Troubleshooting](#-troubleshooting)
- [🤝 Contributing](#-contributing)
- [📄 License](#-license)

---

## 🎯 The Problem

[Claude Code](https://claude.com/claude-code) keeps exactly one login in `~/.claude`. That's fine — until you have a work account, a personal account, and maybe a client's account, all wanting to run the same `claude` command on the same machine.

So this happens:

```
# You've been in a client's codebase all morning.
# Time to check on your own side project.

$ claude
> Continuing as: you@client-corp.com   ← wrong account. again.

# Full re-login required to switch back. Every. Single. Time.
```

The workarounds people reach for all cost something:

| Attempt | Result |
|---|---|
| Logging out and back in each time | A full browser OAuth round-trip, every single switch |
| One shared account for everything | Personal experiments and client work pile into the same history |
| A second OS user account | Now you're switching your whole desktop session for one CLI |
| Manually renaming `~/.claude` | Forgetting to rename it back is how this goes wrong |

**claude-user is the permanent fix.** Register each account once as a profile. Switch with one command. `claude` launches already logged in — no re-auth, no shared history, no manual file shuffling.

---

## 📦 Install

<table>
<tr>
<td width="50%" valign="top">

### npm (recommended)
```sh
npm install -g claude-user
```
Installs the `claude-user` and `cuser` commands. npm pulls in the correct prebuilt binary for your platform automatically — nothing else to configure.

</td>
<td width="50%" valign="top">

### Shell script (Linux / macOS)
```sh
curl -fsSL https://raw.githubusercontent.com/divyo-argha/claude-user/main/install.sh | sh
```
Downloads the latest release straight from GitHub into `~/.local/bin`. Override the target with `CUSER_INSTALL_DIR`, pin a version with `CUSER_VERSION`.

</td>
</tr>
<tr>
<td width="50%" valign="top">

### Cargo / from source
```sh
git clone https://github.com/divyo-argha/claude-user.git
cd claude-user
cargo install --path .
```

</td>
<td width="50%" valign="top">

### Local development
```sh
git clone https://github.com/divyo-argha/claude-user.git
cd claude-user
./scripts/install-local.sh
```

</td>
</tr>
</table>

**Requirements:** [Claude Code](https://claude.com/claude-code) (`claude`) must already be installed and on your `PATH`. `claude-user` doesn't install or manage it — it only launches it with a different config directory.

**Platforms:** Linux (x86_64, aarch64 — glibc) and macOS (Intel, Apple Silicon) have prebuilt binaries. Windows isn't in the release matrix yet; build from source there.

---

## ⬆️ Updating

```sh
claude-user --update
```

Checks GitHub Releases for a newer version, downloads and verifies the matching binary, and replaces the installed `claude-user`/`cuser` binaries in place (needs `tar` on `PATH`, which every supported OS already ships).

```
Checking for updates...
Updating claude-user 0.1.0 → 0.2.0 (linux x86_64)
Updated to claude-user 0.2.0. Downloaded, verified, installed.
```

- If you installed with **npm**, `--update` steps aside instead: npm already owns that binary, so it tells you to run `npm install -g claude-user@latest`.
- If the install directory isn't writable (e.g. a system-wide `cargo install` as another user), it tells you to retry with `sudo`.
- Already up to date? It says so and exits without touching anything.

---

## 🗑️ Uninstall

```sh
# npm install
npm uninstall -g claude-user

# install.sh / cargo install
rm "$(command -v claude-user)" "$(command -v cuser)"
```

Your profiles live independently under `~/.claude-profiles/`. Remove that directory too if you want a clean slate:

```sh
rm -rf ~/.claude-profiles
```

> **Note:** this only removes profiles `claude-user` created. Your original `~/.claude` is left untouched unless you ran `claude-user import`, in which case it was already moved into a profile — nothing is deleted silently.

---

## ⚡ Quick Start

```sh
# Step 1 — bring your current login in as your first profile
claude-user import work
# → moves ~/.claude and ~/.claude.json into a "work" profile, nothing to re-authenticate

# Step 2 — add another account by just naming it
claude-user personal
# → profile doesn't exist yet, so claude-user creates it and launches `claude`,
#   which walks you through login for that account

# Step 3 — switch any time
claude-user work      # back to your work login, instantly
claude-user personal  # back to personal, instantly
```

Prefer arrow keys? Run `claude-user` with no arguments for the interactive picker — it lists every profile plus a "+ Import ~/.claude" option whenever it detects an existing default login, and a "+ New profile" option to start a fresh one.

Anything after a profile name is forwarded straight to `claude`:

```sh
claude-user work --continue
claude-user personal -p "explain this diff"
```

> `cuser` is the short alias — every command on this page works identically as `cuser <profile>` instead of `claude-user <profile>`.

---

## 🏆 Why claude-user?

| Capability | claude-user | Logout + login each time | Second OS user account | Manually renaming `~/.claude` |
|---|:---:|:---:|:---:|:---:|
| One command to switch accounts | ✅ | ❌ | ❌ | ❌ |
| No repeated OAuth login | ✅ | ❌ | ✅ | ✅ |
| Histories stay fully separate | ✅ | ⚠️ only if you remember to log out | ✅ | ⚠️ only if you remember to rename back |
| Works from your normal shell | ✅ | ✅ | ❌ | ✅ |
| Existing login imported automatically | ✅ | – | – | ❌ |
| Shared settings without duplicating auth | ✅ | ❌ | ❌ | ❌ |
| Zero config files to edit by hand | ✅ | ✅ | ✅ | ❌ |

> **The key difference:** claude-user treats each account as a self-contained config directory and hands it to `claude` unmodified — via `CLAUDE_CONFIG_DIR`, the same mechanism Claude Code already supports. It's not a wrapper that reimplements auth; it's a thin, disposable layer around a directory switch.

---

## ✨ Features

<table>
<tr>
<td width="50%" valign="top">

### 👤 Profile Management
- Profiles are created implicitly — `claude-user <name>` makes it if it doesn't exist
- `claude-user list` shows every profile alongside its logged-in email and org, read straight from Claude's own credentials
- `claude-user import [name]` moves your existing `~/.claude` into a new profile — no re-authentication needed
- `claude-user remove <name>` deletes a profile after confirming · `claude-user rename <old> <new>` renames one
- Profile names are validated (letters, digits, `-`, `_` only), so a typo can't touch the wrong directory

</td>
<td width="50%" valign="top">

### 🖥️ Interactive Picker
- Plain `claude-user` opens an arrow-key TUI over every profile
- "+ Import ~/.claude" appears automatically when a default login is detected
- "+ New profile" walks straight into naming and launching a fresh one
- Selecting anything launches `claude` immediately — no second command

</td>
</tr>
<tr>
<td width="50%" valign="top">

### 🔗 Shared Config
- Drop common files (like `settings.json`) into `~/.claude-profiles/shared/`
- `claude-user sync` copies them into every existing profile in one shot
- `.claude.json` and `.credentials.json` are always excluded from sync — a shared-config change can never overwrite a login

</td>
<td width="50%" valign="top">

### 🛡️ Isolation by Default
- Every profile lives at `~/.claude-profiles/<name>/`, its own directory
- Directories are hardened to `0700`, and `.claude.json` / `.credentials.json` to `0600` (Unix)
- Arguments pass straight through — `claude-user work --continue` behaves exactly like `claude --continue`, just scoped to that profile

</td>
</tr>
</table>

---

## 🖥️ Interactive TUI

Launch it with plain `claude-user` (or its short alias, `cuser`) — no subcommand needed:

```
┌ cuser — Claude account switcher ───────────────────────┐
│ ↑/↓ move · Enter select · d delete · r rename · q quit │
├────────────────────────────────────────────────────────┤
│ Profiles                                               │
│ > work        (you@company.com • Acme Corp)            │
│   personal    (you@gmail.com)                          │
│   client-a                                             │
│   + Import ~/.claude                                   │
│   + New profile                                        │
├────────────────────────────────────────────────────────┤
│ Select a profile and press Enter.                      │
└────────────────────────────────────────────────────────┘
```

Picking **+ New profile** or **+ Import ~/.claude** drops into a one-line name prompt, then launches straight into `claude`. Highlighting an existing profile also offers `d` to delete it (with an inline confirmation) and `r` to rename it — no CLI required.

### ⌨️ Keyboard Navigation

| Key | Action |
|---|---|
| <kbd>↑</kbd> / <kbd>k</kbd> | Move selection up |
| <kbd>↓</kbd> / <kbd>j</kbd> | Move selection down |
| <kbd>Enter ↵</kbd> | Select profile, or confirm a new name |
| <kbd>d</kbd> | Delete the highlighted profile (asks to confirm) |
| <kbd>r</kbd> | Rename the highlighted profile |
| <kbd>y</kbd> / <kbd>n</kbd> | Confirm or cancel a pending delete |
| <kbd>Esc</kbd> | Cancel naming/renaming and return to the list |
| <kbd>q</kbd> | Quit without launching anything |
| <kbd>Ctrl</kbd>+<kbd>C</kbd> | Quit immediately, from anywhere |
| <kbd>Backspace</kbd> | Edit the name you're typing |

The picker needs a real interactive terminal — it enables raw mode and draws to an alternate screen, so it won't work piped through another program or inside a non-TTY script.

---

## 🔄 How It Works

### Under the hood — one switch

```
claude-user work
    │
    ▼
1. Looks up "work" under ~/.claude-profiles/
2. Creates the profile directory (0700) the first time, and copies
   ~/.claude-profiles/shared/ into it, skipping auth files
3. Sets CLAUDE_CONFIG_DIR=~/.claude-profiles/work
4. execs `claude`, passing through any extra arguments
        │
        ▼
claude reads all of its state — credentials, settings, project
history — from CLAUDE_CONFIG_DIR. Pointing it at a different
directory per profile gives each account a fully separate
environment, with no extra moving parts — switching never touches
the network itself (only `claude-user --update` does, separately).
```

### A day with multiple accounts

```
 9:00 AM — starting work
──────────────────────────────────────────────────────
 $ claude-user work
   → claude launches already signed in as you@company.com

 1:00 PM — open source on a break
──────────────────────────────────────────────────────
 $ claude-user personal
   → separate history, separate login, as you@gmail.com

 5:00 PM — a client engagement
──────────────────────────────────────────────────────
 $ claude-user client-a
   → its own isolated profile — nothing crosses over
```

No re-login. No shared history. No file to remember to rename back.

---

## 🛡️ Security

<table>
<tr>
<td width="50%" valign="top">

**What claude-user does**
- Creates every profile directory at `0700`, and hardens `.claude.json` / `.credentials.json` inside it to `0600` (Unix)
- Excludes `.claude.json` and `.credentials.json` from `claude-user sync`, so shared config can never overwrite a login
- Requires an explicit confirmation — `[y/N]` on the CLI, `y`/`Y` in the TUI — before `remove` deletes a profile's stored credentials
- Validates profile names against a strict allow-list before touching the filesystem
- `--update` verifies the downloaded binary's reported version before replacing anything already installed
- Uses an atomic rename (falling back to copy-then-remove) when importing your existing `~/.claude`

</td>
<td width="50%" valign="top">

**What claude-user never does**
- Never talks to the network on your behalf during normal use — `list`, `sync`, `import`, and launching a profile are all local filesystem operations; only `claude-user --update` reaches out, and only to `github.com`/`api.github.com` to check for and download a new release
- Never modifies anything outside `~/.claude-profiles`, except your default `~/.claude` / `~/.claude.json` (only when you explicitly run `import`) and its own installed binaries (only when you explicitly run `--update`)
- Never invents new credential storage — each profile's `.credentials.json` is the same file Claude Code already writes, just isolated per account

</td>
</tr>
</table>

---

## 📋 Commands

> Every command below also works with the short alias `cuser` in place of `claude-user`.

| Command | Description |
|---|---|
| `claude-user` | Open the interactive profile picker |
| `claude-user <profile>` | Launch that profile (created automatically if it's new) |
| `claude-user <profile> [args...]` | Launch that profile, forwarding `[args]` straight to `claude` |
| `claude-user list` / `-l` | List every profile, with email/org where known |
| `claude-user sync` | Copy `~/.claude-profiles/shared/` into every existing profile |
| `claude-user import [name]` | Import your currently logged-in `~/.claude` as a new profile |
| `claude-user remove <profile>` | Delete a profile — asks for confirmation first, since it deletes stored credentials |
| `claude-user rename <old> <new>` | Rename a profile |
| `claude-user --update` | Update to the latest release (see [Updating](#-updating)) |
| `claude-user --version` / `-v` | Show the installed version |
| `claude-user --help` / `-h` | Show usage |

**Aliases:** `cuser` → short alias for `claude-user` (identical binary, works everywhere above) · `migrate` → alias for `import` · `rm` / `delete` → aliases for `remove` · `update` → alias for `--update`

---

## 📁 Directory Layout

```
~/.claude-profiles/
  ├── shared/                 ← common config, copied into every profile by `sync`
  ├── work/                   ← one directory per profile (0700)
  │   ├── .claude.json        ← onboarding + oauth account info (0600)
  │   ├── .credentials.json   ← Claude Code auth tokens (0600)
  │   └── settings.json, ...  ← everything else `claude` stores per config dir
  ├── personal/
  └── client-a/
```

Nothing under `~/.claude` is touched except by `claude-user import`, and even then it's moved — not copied and left behind.

---

## 🔧 Troubleshooting

| Symptom | Fix |
|---|---|
| `claude-user: command not found` after `npm install -g` | npm's global bin directory isn't on your `PATH` — run `npm config get prefix` and add `<prefix>/bin` to your shell profile |
| `failed to launch claude (is it installed and on PATH?)` | Install [Claude Code](https://claude.com/claude-code) first — `claude-user` only switches accounts, it doesn't install `claude` itself |
| `claude-user does not ship a prebuilt binary for ...` | Your OS/architecture isn't in the current release matrix (Windows, for example) — build from source with `cargo install --path .` |
| Picker draws garbled or doesn't respond | The TUI needs a real interactive terminal (it uses raw mode) — it won't work piped through another program or in a non-TTY shell |
| `claude-user import` fails with "profile already exists" | Pick a different name — `import` never overwrites an existing profile |
| `--update` says "managed by npm" | Expected — run `npm install -g claude-user@latest` instead. Self-update only replaces binaries installed via `install.sh` or `cargo install` |
| `--update` fails with "permission denied" | Retry with `sudo claude-user --update`, or reinstall to a directory your user owns |

---

## 🤝 Contributing

Issues and pull requests are welcome. If something's broken, open an issue — if something's just confusing, that's worth filing too.

```sh
git clone https://github.com/divyo-argha/claude-user.git
cd claude-user
cargo build
cargo run --bin cuser -- --help
```

---

## 📄 License

MIT — see [LICENSE](LICENSE).

---

<div align="center">

**For everyone running Claude Code under more than one hat.**

<br />

[![GitHub](https://img.shields.io/badge/Star%20on%20GitHub-181717?style=flat&logo=github&logoColor=white)](https://github.com/divyo-argha/claude-user)
[![npm](https://img.shields.io/badge/Install%20via%20npm-CB3837?style=flat&logo=npm&logoColor=white)](https://www.npmjs.com/package/claude-user)

<br />

<sub>If claude-user saved you a login, consider giving it a ⭐</sub>

</div>
