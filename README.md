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
    <a href="#🚀-quick-onboarding">Quick Onboarding</a> •
    <a href="#📦-installation">Installation</a> •
    <a href="#📋-command-options">Command Options</a> •
    <a href="#🖥️-interactive-tui">Interactive TUI</a> •
    <a href="#🛡️-security--isolation">Security</a> •
    <a href="#🔧-troubleshooting">Troubleshooting</a>
  </p>

  <br />

  <img src="https://img.shields.io/badge/Linux-supported-FCC624?style=flat&logo=linux&logoColor=black" alt="Linux" />
  <img src="https://img.shields.io/badge/macOS-supported-000000?style=flat&logo=apple&logoColor=white" alt="macOS" />
  <img src="https://img.shields.io/badge/Windows-build%20from%20source-lightgrey?style=flat&logo=windows&logoColor=white" alt="Windows" />

  <br /><br />
</div>

---

## 🎯 The Problem

[Claude Code](https://claude.com/claude-code) keeps exactly one login in `~/.claude`. When you have a work account, a personal account, and client accounts, you are forced to constantly log out, log in, and do OAuth round-trips.

**`claude-user` (aliased as `cuser`) is the fix.** Register each account once as a profile. Switch with one command. `claude` launches already logged in — no re-auth, no shared history, no manual file shuffling.

---

## 📋 Prerequisites

Before getting started, make sure you have installed the official **Claude Code** CLI:

```bash
npm install -g @anthropic-ai/claude-code
```

> [!NOTE]
> `claude-user` is a helper to switch account environments; it does not replace or install the `claude` command itself.

---

## 🚀 Quick Onboarding

Get up and running with multiple profiles in under a minute:

### 1. Import your current logged-in account
If you already have a logged-in session, import it as your first profile (e.g., named `work`):
```bash
cuser import work
```
*This moves your existing `~/.claude` session files into a profile named `work`. You won't need to log in again.*

### 2. Add your second account
Create a brand new profile (e.g., named `personal`) and log in:
```bash
cuser personal
```
*Since the `personal` profile does not exist yet, `cuser` creates it and launches the Claude onboarding login flow. Complete the OAuth login once.*

### 3. Switch instantly
Now switch between your accounts anytime by passing the profile name:
```bash
cuser work
cuser personal
```
*Every time you run a profile command, it swaps the symlinks. Your bare `claude` command will also automatically use the last activated profile!*

---

## 📦 Installation

Choose one of the methods below to install the `claude-user` and `cuser` commands:

### Method A: npm (Recommended)
```bash
npm install -g claude-user
```
*Pulls in the correct prebuilt binary for your platform automatically.*

### Method B: Shell Script (Linux / macOS)
```bash
curl -fsSL https://raw.githubusercontent.com/divyo-argha/claude-user/main/install.sh | sh
```
*Downloads the latest release into `~/.local/bin`.*

### Method C: Cargo / Build from Source
```bash
git clone https://github.com/divyo-argha/claude-user.git
cd claude-user
cargo install --path .
```

---

## 📋 Command Options

You can invoke `claude-user` using the full command or the short alias `cuser` (which works identically).

### Primary Actions
| Command | Short / Alias | Description | Example |
| :--- | :--- | :--- | :--- |
| `cuser` | | Open the interactive console profile picker | `cuser` |
| `cuser <profile>` | | Launch Claude with this profile (creates it if new) | `cuser work` |
| `cuser <profile> [args...]` | | Launch profile, forwarding all remaining arguments to Claude | `cuser personal --continue` |

### Profile & Config Management
| Command | Alias / Alternates | Description | Example |
| :--- | :--- | :--- | :--- |
| `cuser import [name]` | `cuser migrate [name]` | Import current `~/.claude` credentials as a profile | `cuser import work` |
| `cuser list` | `cuser -l` | List all profiles with linked email/org information | `cuser list` |
| `cuser rename <old> <new>` | | Rename an existing profile | `cuser rename main work` |
| `cuser remove <profile>` | `cuser rm`, `cuser delete` | Delete profile directory and stored credentials (asks confirmation) | `cuser remove personal` |
| `cuser sync` | | Sync files from `shared/` directory into all profiles | `cuser sync` |

### System Commands
| Command | Short / Alias | Description |
| :--- | :--- | :--- |
| `cuser --update` | `cuser update` | Check and update to the latest release version |
| `cuser --version` | `cuser -v` | Show installed version |
| `cuser --help` | `cuser -h` | Show help message |

---

## 🖥️ Interactive TUI

If you run `cuser` with no arguments, it opens the interactive Terminal User Interface:

```
┌ cuser — Claude account switcher ───────────────────────┐
│ ↑/↓ move · Enter select · d delete · r rename · q quit │
├────────────────────────────────────────────────────────┤
│ Profiles                                               │
│ > work        (you@company.com • Acme Corp)            │
│   personal    (you@gmail.com)                          │
│   + Import ~/.claude                                   │
│   + New profile                                        │
├────────────────────────────────────────────────────────┤
│ Select a profile and press Enter.                      │
└────────────────────────────────────────────────────────┘
```

### Keyboard Navigation
* <kbd>↑</kbd> or <kbd>k</kbd> / <kbd>↓</kbd> or <kbd>j</kbd>: Navigate through profiles
* <kbd>Enter ↵</kbd>: Select and launch the profile
* <kbd>r</kbd>: Rename the highlighted profile
* <kbd>d</kbd>: Delete the highlighted profile (will ask for `y`/`n` confirmation)
* <kbd>q</kbd> or <kbd>Esc</kbd>: Exit the picker

---

## 🛡️ Security & Isolation

* **Strict Directory Permissions:** Profiles are created under `~/.claude-profiles/` with `0700` (read/write/execute by owner only) permissions. Sensitive credentials (`.credentials.json` and `.claude.json`) are hardened to `0600` permissions.
* **Safe Syncing:** The `cuser sync` command always ignores credential and onboarding files (`.credentials.json`, `.claude.json`), preventing accidental overwrite of auth tokens.
* **Local Operation:** `claude-user` runs entirely offline and locally. The only network call it makes is checking for software updates when you explicitly run `cuser --update`.

---

## 🔧 Troubleshooting

| Issue / Error | Solution |
| :--- | :--- |
| `~/.claude exists and isn't managed by cuser` | Your `~/.claude` is a real folder, not a symlink. Import it using `cuser import default` or move it out of the way with `mv ~/.claude ~/.claude.bak`. |
| `failed to launch claude (is it installed and on PATH?)` | You must install the official Claude Code package first (`npm install -g @anthropic-ai/claude-code`). `cuser` is a profile manager, not a Claude replacement. |
| `cuser: command not found` | If installed globally via npm, ensure your global npm bin directory is added to your shell's `PATH`. |
| Picker draws garbled / doesn't respond | The TUI requires an interactive terminal (TTY) with raw mode support. It will not work inside pipes, non-interactive shells, or some minimal IDE terminals. |
