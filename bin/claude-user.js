#!/usr/bin/env node
'use strict';

const path = require('path');
const { spawnSync } = require('child_process');

const PLATFORMS = {
  'linux-x64': 'claude-user-linux-x64',
  'linux-arm64': 'claude-user-linux-arm64',
  'darwin-x64': 'claude-user-darwin-x64',
  'darwin-arm64': 'claude-user-darwin-arm64',
};

function resolveBinary() {
  const key = `${process.platform}-${process.arch}`;
  const pkgName = PLATFORMS[key];

  if (!pkgName) {
    const isWindows = process.platform === 'win32';
    const instructions = isWindows
      ? `1. Install Rust by downloading and running: https://rustup.rs/\n` +
        `2. Build and install the tool via PowerShell/Command Prompt:\n` +
        `   git clone https://github.com/divyo-argha/claude-user.git\n` +
        `   cd claude-user\n` +
        `   cargo install --path .`
      : `1. Install Rust:\n` +
        `   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n` +
        `2. Build and install the tool:\n` +
        `   git clone https://github.com/divyo-argha/claude-user.git\n` +
        `   cd claude-user\n` +
        `   cargo install --path .`;

    throw new Error(
      `claude-user does not ship a prebuilt binary for ${process.platform}/${process.arch}.\n\n` +
      `To build and install from source, follow these steps:\n\n${instructions}`
    );
  }

  let pkgJsonPath;
  try {
    pkgJsonPath = require.resolve(`${pkgName}/package.json`);
  } catch {
    throw new Error(
      `Could not find the "${pkgName}" package.\n` +
        `It should have been installed automatically as an optional dependency of claude-user.\n` +
        `Try reinstalling: npm install claude-user --force`
    );
  }

  return path.join(path.dirname(pkgJsonPath), 'bin', 'cuser');
}

function main() {
  const binPath = resolveBinary();
  const result = spawnSync(binPath, process.argv.slice(2), { stdio: 'inherit' });

  if (result.error) {
    if (result.error.code === 'ENOENT') {
      console.error(`claude-user: could not execute "${binPath}"`);
    } else {
      console.error(result.error.message);
    }
    process.exit(1);
  }

  if (result.signal) {
    process.kill(process.pid, result.signal);
    return;
  }

  process.exit(result.status === null ? 1 : result.status);
}

try {
  main();
} catch (err) {
  console.error(err.message);
  process.exit(1);
}
