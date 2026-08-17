#!/usr/bin/env node
'use strict';

// Generates the per-platform optionalDependency packages under npm/packages/.
// Run by CI right before `npm publish` — never committed, never run as an npm
// lifecycle script. Expects the release-matrix tarballs (built by
// .github/workflows/release.yml) to already be extracted-or-present under dist/,
// one dist/claude-user-<rust-target>.tar.gz per platform.

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const rootDir = path.join(__dirname, '..');
const distDir = path.join(rootDir, 'dist');
const packagesDir = path.join(__dirname, 'packages');

const TARGETS = [
  { rustTarget: 'x86_64-unknown-linux-gnu', nodeOs: 'linux', nodeArch: 'x64', libc: 'glibc' },
  { rustTarget: 'aarch64-unknown-linux-gnu', nodeOs: 'linux', nodeArch: 'arm64', libc: 'glibc' },
  { rustTarget: 'x86_64-apple-darwin', nodeOs: 'darwin', nodeArch: 'x64' },
  { rustTarget: 'aarch64-apple-darwin', nodeOs: 'darwin', nodeArch: 'arm64' },
];

const rootPkg = JSON.parse(fs.readFileSync(path.join(rootDir, 'package.json'), 'utf8'));
const { version, license, author, repository, bugs, homepage, engines } = rootPkg;

console.log(`Building platform packages for version ${version}...`);

fs.rmSync(packagesDir, { recursive: true, force: true });
fs.mkdirSync(packagesDir, { recursive: true });

for (const target of TARGETS) {
  const pkgName = `claude-user-${target.nodeOs}-${target.nodeArch}`;
  const pkgDir = path.join(packagesDir, pkgName);
  const pkgBinDir = path.join(pkgDir, 'bin');
  fs.mkdirSync(pkgBinDir, { recursive: true });

  const tarball = path.join(distDir, `claude-user-${target.rustTarget}.tar.gz`);
  if (!fs.existsSync(tarball)) {
    console.error(`Missing build artifact: ${tarball}`);
    process.exit(1);
  }
  console.log(`Staging binary for ${pkgName}...`);
  execFileSync('tar', ['-xzf', tarball, '-C', pkgBinDir, 'cuser'], { stdio: 'inherit' });
  fs.chmodSync(path.join(pkgBinDir, 'cuser'), 0o755);

  const subPkgJson = {
    name: pkgName,
    version,
    description: `${target.nodeOs} ${target.nodeArch} binary for claude-user — installed automatically as an optional dependency, not meant to be installed directly.`,
    author,
    license,
    repository: { ...repository, directory: `npm/packages/${pkgName}` },
    bugs,
    homepage,
    engines,
    os: [target.nodeOs],
    cpu: [target.nodeArch],
    ...(target.libc ? { libc: [target.libc] } : {}),
    files: ['bin'],
    publishConfig: { access: 'public' },
  };
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify(subPkgJson, null, 2) + '\n');

  const readme = `# ${pkgName}\n\nPlatform-specific binary package for [\`claude-user\`](https://www.npmjs.com/package/claude-user).\nIt is installed automatically as an optional dependency — you should not install it directly.\n\nSee [github.com/divyo-argha/claude-user](https://github.com/divyo-argha/claude-user) for usage.\n`;
  fs.writeFileSync(path.join(pkgDir, 'README.md'), readme);

  const licensePath = path.join(rootDir, 'LICENSE');
  if (fs.existsSync(licensePath)) {
    fs.copyFileSync(licensePath, path.join(pkgDir, 'LICENSE'));
  }
}

console.log('Successfully built all packages under npm/packages/.');
