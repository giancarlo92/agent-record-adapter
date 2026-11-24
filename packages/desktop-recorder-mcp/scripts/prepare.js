// Part of Agent Record Adapter (see NOTICE).
'use strict';

// Puts the programs, the skills and the licences into the package before packing it:
// - agent-record.exe (the front) and agent-desktop.exe (the engine) from this repository's
//   release build (cargo build --release at the repository root);
// - agent-browser.exe built from the unmodified agent-browser submodule, into a target folder of
//   our own (cargo build --release --manifest-path vendor/agent-browser/cli/Cargo.toml
//   --target-dir target/agent-browser), so nothing is installed from npm;
// - desktop-recorder.exe from apps/recorder (cargo build --release in that folder);
// - our skills from skills/, and the official agent-desktop and agent-browser skills copied
//   unmodified from the submodules under vendor/;
// - the bundled agent-browser version, taken from the submodule.

const fs = require('fs');
const path = require('path');

const pkg = path.join(__dirname, '..');
const repo = path.join(pkg, '..', '..');
const target = process.env.CARGO_TARGET_DIR || path.join(repo, 'target');
const appTarget = process.env.RECORDER_TARGET_DIR || path.join(repo, 'apps', 'recorder', 'target');
const { OFFICIAL_SKILLS, OUR_SKILLS } = require('../lib/install');

const programs = [
  [path.join(target, 'release', 'agent-record.exe'), 'cargo build --release -p agent-record'],
  [path.join(target, 'release', 'agent-desktop.exe'), 'cargo build --release -p agent-record-engine'],
  [path.join(target, 'agent-browser', 'release', 'agent-browser.exe'),
    'cargo build --release --manifest-path vendor/agent-browser/cli/Cargo.toml --target-dir target/agent-browser'],
  [path.join(appTarget, 'release', 'desktop-recorder.exe'), 'cargo build --release in apps/recorder'],
];
for (const [exe, how] of programs) {
  if (!fs.existsSync(exe)) {
    console.error(`No release build at ${exe}: run ${how} first`);
    process.exit(1);
  }
}
fs.mkdirSync(path.join(pkg, 'vendor'), { recursive: true });
for (const [exe] of programs) fs.copyFileSync(exe, path.join(pkg, 'vendor', path.basename(exe)));

function copyDir(from, to) {
  fs.mkdirSync(to, { recursive: true });
  for (const entry of fs.readdirSync(from, { withFileTypes: true })) {
    const a = path.join(from, entry.name);
    const b = path.join(to, entry.name);
    if (entry.isDirectory()) copyDir(a, b);
    else fs.copyFileSync(a, b);
  }
}

const official = {
  'agent-desktop': path.join(repo, 'vendor', 'agent-desktop', 'skills', 'agent-desktop'),
  'agent-desktop-windows': path.join(repo, 'vendor', 'agent-desktop', 'skills', 'agent-desktop-windows'),
  'agent-browser': path.join(repo, 'vendor', 'agent-browser', 'skills', 'agent-browser'),
};
for (const skill of [...OFFICIAL_SKILLS, ...OUR_SKILLS]) {
  const from = official[skill] || path.join(repo, 'skills', skill);
  fs.rmSync(path.join(pkg, 'skills', skill), { recursive: true, force: true });
  copyDir(from, path.join(pkg, 'skills', skill));
}

const manifest = path.join(pkg, 'package.json');
const data = JSON.parse(fs.readFileSync(manifest, 'utf8'));
data.bundled = { 'agent-browser': require(path.join(repo, 'vendor', 'agent-browser', 'package.json')).version };
fs.writeFileSync(manifest, JSON.stringify(data, null, 2) + '\n');

for (const file of ['LICENSE', 'NOTICE', 'THIRD-PARTY-LICENSES.txt']) {
  fs.copyFileSync(path.join(repo, file), path.join(pkg, file));
}
console.log(`vendor/ (${programs.length} programs), skills/ and licences ready; agent-browser ${data.bundled['agent-browser']}`);
