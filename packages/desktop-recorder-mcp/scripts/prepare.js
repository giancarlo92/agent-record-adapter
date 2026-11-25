// Part of Agent Record Adapter (see NOTICE).
'use strict';

// Puts the programs, the skills and the licences into the package before packing it:
// - agent-record.exe from this repository's release build (cargo build --release -p agent-record
//   at the repository root). It carries inside the agent-desktop engine (with our adapter) and
//   agent-browser, both built from their unmodified submodules under vendor/, so nothing is
//   installed from npm and there is no other program of theirs to ship;
// - desktop-recorder.exe from apps/recorder, built here with the SHA-256 of that agent-record.exe
//   (AGENT_RECORD_SHA256), so the app refuses an agent-record.exe that was changed;
// - our skills from skills/, and the official agent-desktop and agent-browser skills copied
//   unmodified from the submodules under vendor/;
// - the bundled agent-browser version, taken from the submodule.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');

const pkg = path.join(__dirname, '..');
const repo = path.join(pkg, '..', '..');
const target = process.env.CARGO_TARGET_DIR || path.join(repo, 'target');
const appDir = path.join(repo, 'apps', 'recorder');
const appTarget = process.env.RECORDER_TARGET_DIR || path.join(appDir, 'target');
const { OFFICIAL_SKILLS, OUR_SKILLS } = require('../lib/install');

const front = path.join(target, 'release', 'agent-record.exe');
if (!fs.existsSync(front)) {
  console.error(`No release build at ${front}: run cargo build --release -p agent-record first`);
  process.exit(1);
}
const sha256 = crypto.createHash('sha256').update(fs.readFileSync(front)).digest('hex');
execFileSync('cargo', ['build', '--release'], {
  cwd: appDir,
  stdio: 'inherit',
  env: { ...process.env, AGENT_RECORD_SHA256: sha256, CARGO_TARGET_DIR: appTarget },
});
const programs = [front, path.join(appTarget, 'release', 'desktop-recorder.exe')];
const vendorDir = path.join(pkg, 'vendor');
fs.mkdirSync(vendorDir, { recursive: true });
// Programs an earlier build of the package shipped apart.
for (const old of ['agent-desktop.exe', 'agent-browser.exe']) fs.rmSync(path.join(vendorDir, old), { force: true });
for (const exe of programs) fs.copyFileSync(exe, path.join(vendorDir, path.basename(exe)));

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
console.log(`vendor/ (${programs.length} programs), skills/ and licences ready; agent-browser ${data.bundled['agent-browser']} inside agent-record`);
