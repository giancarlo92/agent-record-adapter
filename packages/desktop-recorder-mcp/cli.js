#!/usr/bin/env node
// Part of Agent Record Adapter (see NOTICE).
'use strict';

// npx desktop-recorder-mcp            install: pick your agents from a list
// npx desktop-recorder-mcp status     what is installed, and where
// npx desktop-recorder-mcp uninstall  remove it from your agents
// npx desktop-recorder-mcp app        open Desktop Recorder (record a task, play it again)
// npx desktop-recorder-mcp update     this version, for the agents already set up (Desktop Recorder runs it)

const os = require('os');
const path = require('path');
const { agents } = require('./lib/agents');
const { install, installedAgents, uninstall, status, layout } = require('./lib/install');
const { multiselect, confirm } = require('./lib/prompt');

const HELP = `desktop-recorder-mcp: Agent Record for your AI agents, in one package: agent-desktop and agent-browser
(built from their unmodified sources, nothing else installed), plus background mode, record
(events, video, an on-screen indicator), saved passwords and Desktop Recorder

Usage:
  npx desktop-recorder-mcp [install] [options]   install and set up the agents you pick
  npx desktop-recorder-mcp status                 what is installed, and where
  npx desktop-recorder-mcp uninstall [options]   remove it from the agents you pick
  npx desktop-recorder-mcp app                    open Desktop Recorder: record a task and play it again, without AI
  npx desktop-recorder-mcp@latest update          install this version for the agents already set up, without asking

Options:
  --agents a,b   the agents to set up, without asking: ${'claude-code, codex, gemini, claude-desktop, cursor, vscode, opencode'}
  --all          every agent found on this computer, without asking
  --dry-run      say what would change, change nothing
  --home DIR     act on another home folder (for tests)
  --remove-program   with uninstall: also delete the programs (recordings and saved passwords are kept)
  --restart-app  with update: open Desktop Recorder again when it is done
  -h, --help     this help`;

function parse(argv) {
  const options = { command: 'install', agents: null, all: false, dryRun: false, home: null, removeProgram: false, restartApp: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (['install', 'status', 'uninstall', 'app', 'update'].includes(arg)) options.command = arg;
    else if (arg === '--restart-app') options.restartApp = true;
    else if (arg === '--agents') options.agents = (argv[++i] || '').split(',').map((s) => s.trim()).filter(Boolean);
    else if (arg === '--all') options.all = true;
    else if (arg === '--dry-run') options.dryRun = true;
    else if (arg === '--home') options.home = argv[++i];
    else if (arg === '--remove-program') options.removeProgram = true;
    else if (arg === '-h' || arg === '--help') options.command = 'help';
    else throw new Error(`Unknown option ${arg}; see --help`);
  }
  return options;
}

/**
 * Installs this version for the agents that already have it, without asking: what Desktop
 * Recorder's update button runs. It writes how it went where the app reads it, and opens the app
 * again (the new one, or the one there was when the update failed).
 */
async function update(home, run, restartApp) {
  const where = layout(home);
  const resultFile = path.join(where.root, 'update-result.json');
  const version = require('./package.json').version;
  let result;
  try {
    // The app closes itself after starting this; give it a moment to let go of its file.
    if (restartApp) await new Promise((done) => setTimeout(done, 1500));
    const lines = install(installedAgents(run), { ...run, log: console.log });
    lines.forEach((line) => console.log(line));
    result = { ok: true, version, at: Date.now() };
  } catch (error) {
    console.error(`desktop-recorder-mcp: ${error.message}`);
    result = { ok: false, version, error: error.message, at: Date.now() };
    process.exitCode = 1;
  }
  require('fs').mkdirSync(where.root, { recursive: true });
  require('fs').writeFileSync(resultFile, JSON.stringify(result));
  if (restartApp) openApp(home);
}

/** Opens the installed recorder app, without waiting for it. */
function openApp(home) {
  const app = layout(home).app;
  if (!require('fs').existsSync(app)) {
    console.log('Desktop Recorder is not installed yet: run npx desktop-recorder-mcp first.');
    process.exitCode = 1;
    return;
  }
  require('child_process').spawn(app, [], { detached: true, stdio: 'ignore' }).unref();
  console.log(`Desktop Recorder: ${app}`);
}

async function main() {
  const options = parse(process.argv.slice(2));
  if (options.command === 'help') return console.log(HELP);
  if (process.platform !== 'win32' && !options.home) {
    console.log('desktop-recorder-mcp runs on Windows for now. macOS and Linux are coming soon.');
    process.exitCode = 1;
    return;
  }
  const home = options.home || os.homedir();
  // Another home (tests) keeps everything there, the Start menu entry too.
  const env = options.home
    ? { ...process.env, APPDATA: path.join(home, 'AppData', 'Roaming'), LOCALAPPDATA: path.join(home, 'AppData', 'Local') }
    : process.env;
  const run = { home, env, dryRun: options.dryRun, useCli: !options.home, removeProgram: options.removeProgram };
  if (options.command === 'status') return status(run).forEach((line) => console.log(line));
  if (options.command === 'app') return openApp(home);
  if (options.command === 'update') return update(home, run, options.restartApp);

  const known = agents(home, env);
  const found = known.filter((a) => a.detect());
  let chosen = options.agents;
  if (chosen) {
    const ids = known.map((a) => a.id);
    const unknown = chosen.filter((id) => !ids.includes(id));
    if (unknown.length) throw new Error(`Unknown agent ${unknown.join(', ')}; use ${ids.join(', ')}`);
  } else if (options.all) {
    chosen = found.map((a) => a.id);
  } else if (process.stdin.isTTY) {
    const verb = options.command === 'uninstall' ? 'remove it from' : 'set it up in';
    chosen = await multiselect(`Which agents do you want to ${verb}?`, known.map((a) => ({
      label: a.name,
      value: a.id,
      checked: options.command === 'install' && a.detect(),
      note: a.detect() ? '' : '(not found)',
    })));
  } else {
    throw new Error('No terminal to ask in: pass --agents claude-code,codex or --all');
  }
  if (!chosen.length) return console.log('Nothing chosen, nothing changed.');

  if (options.command === 'uninstall') {
    return uninstall(chosen, run).forEach((line) => console.log(line));
  }
  if (!options.dryRun && process.stdin.isTTY && !options.agents && !options.all) {
    const where = layout(home);
    const ok = await confirm(`Install the program in ${where.bin} and set up ${chosen.length} agent(s)?`, true);
    if (!ok) return console.log('Nothing changed.');
  }
  install(chosen, { ...run, log: console.log }).forEach((line) => console.log(line));
  if (!options.dryRun) {
    console.log('\nOpen a new conversation in each agent and ask it: "What displays do I have?"');
    console.log('To record: "Record the Excel window with video and save it in D:\\Videos\\task.mp4".');
  }
}

main().catch((error) => {
  console.error(`desktop-recorder-mcp: ${error.message}`);
  process.exitCode = 1;
});
