// Part of Agent Record Adapter (see NOTICE).
'use strict';

// Installing Agent Record Adapter: the programs in one folder of their own (agent-record, the front;
// agent-desktop, the engine built from the unmodified upstream sources with our adapter; Desktop
// Recorder), the official agent-browser npm package unmodified in a folder of its own, then for each
// chosen agent the MCP server and, where the agent reads skills from a folder, the skills.

const fs = require('fs');
const path = require('path');
const { execFileSync, execSync } = require('child_process');

const config = require('./config');
const { agents, onPath } = require('./agents');

const SERVER = 'agent-record';
const PACKAGE = path.join(__dirname, '..');
// The official skills (copied unmodified from the upstream projects) and ours, which complement them.
const OFFICIAL_SKILLS = ['agent-desktop', 'agent-desktop-windows', 'agent-browser'];
const OUR_SKILLS = ['agent-record-background', 'desktop-recording', 'recording-to-script', 'script-repair'];
const MARKER = '.agent-record-adapter';

/** The built programs: the package's vendor folder, or AGENT_RECORD_ADAPTER_VENDOR (tests). */
function vendorDir() {
  return process.env.AGENT_RECORD_ADAPTER_VENDOR || path.join(PACKAGE, 'vendor');
}

/** The skills folder: the package's, or AGENT_RECORD_ADAPTER_SKILLS (tests and a repository checkout). */
function skillsDir() {
  return process.env.AGENT_RECORD_ADAPTER_SKILLS || path.join(PACKAGE, 'skills');
}
const SKILLS = [...OFFICIAL_SKILLS, ...OUR_SKILLS];

/** Where everything goes for one home folder. */
function layout(home) {
  const root = path.join(home, MARKER);
  return {
    root,
    bin: path.join(root, 'bin'),
    // The front every agent and script runs: agent-desktop's commands plus ours, and the MCP server.
    exe: path.join(root, 'bin', 'agent-record.exe'),
    // The engine the front delegates agent-desktop's own commands to.
    engine: path.join(root, 'bin', 'agent-desktop.exe'),
    // The small recorder app (Record / Stop, its recordings, Play), next to agent-record.
    app: path.join(root, 'bin', 'desktop-recorder.exe'),
    // The official agent-browser npm package, installed unmodified with npm.
    browser: path.join(root, 'browser'),
    // The tool's private state (refs, snapshots, recordings), apart from the upstream tool's ~/.agent-desktop.
    state: path.join(root, 'state'),
  };
}

function server(where) {
  const env = { AGENT_DESKTOP_HOME: where.state };
  const browser = agentBrowserPath(where);
  if (browser) env.AGENT_BROWSER_BIN = browser;
  return { command: where.exe, args: ['mcp'], env };
}

/** The installed agent-browser: its native program when npm placed it, else npm's command script. */
function agentBrowserPath(where) {
  const modules = path.join(where.browser, 'node_modules');
  const arch = process.arch === 'arm64' ? 'x64' : process.arch;
  const candidates = [
    process.platform === 'win32' ? path.join(modules, 'agent-browser', 'bin', `agent-browser-win32-${arch}.exe`) : null,
    path.join(modules, '.bin', process.platform === 'win32' ? 'agent-browser.cmd' : 'agent-browser'),
  ].filter(Boolean);
  return candidates.find((file) => fs.existsSync(file)) || null;
}

/** The agent-browser version this package was built and tested with. */
function agentBrowserVersion() {
  return (require('../package.json').agentBrowser || {}).version || 'latest';
}

/** The npm command line that installs the official agent-browser, unmodified, into its own folder. */
function browserInstallCommand(where) {
  return commandLine(['npm', 'install', '--prefix', where.browser, '--no-fund', '--no-audit', '--no-save',
    `agent-browser@${agentBrowserVersion()}`]);
}

function installBrowser(where, options, log) {
  const line = browserInstallCommand(where);
  if (!line) throw new Error(`it cannot be installed into ${where.browser} from the command shell`);
  fs.mkdirSync(where.browser, { recursive: true });
  execSync(line, { stdio: 'ignore', windowsHide: true, env: options.env, cwd: where.browser });
  const found = agentBrowserPath(where);
  if (!found) throw new Error('npm installed it but its program was not found');
  log(`agent-browser ${agentBrowserVersion()}: ${found}`);
}

function installProgram(where, log) {
  const vendor = vendorDir();
  const source = path.join(vendor, 'agent-record.exe');
  const engine = path.join(vendor, 'agent-desktop.exe');
  for (const file of [source, engine]) {
    if (!fs.existsSync(file)) throw new Error(`The package has no program (${file}); run npm run prepare-vendor`);
  }
  fs.mkdirSync(where.bin, { recursive: true });
  fs.mkdirSync(where.state, { recursive: true });
  makePrivate(where.state);
  removeSetAside(where.bin);
  const kept = [replace(source, where.exe), replace(engine, where.engine)].some(Boolean);
  const app = path.join(vendor, 'desktop-recorder.exe');
  if (fs.existsSync(app)) replace(app, where.app);
  // The recorder app reads it to know which version it is, and whether a newer one is out.
  fs.writeFileSync(path.join(where.bin, 'version.txt'), require('../package.json').version);
  log(`Program: ${where.exe}`);
  if (kept) log('Agents that were open keep the previous version until they restart.');
}

/** The agents that have this MCP server set up now (what an update sets up again). */
function installedAgents(options) {
  return agents(options.home, options.env)
    .filter((agent) => {
      try {
        return config.has(agent.mcp, SERVER);
      } catch (_) {
        return false;
      }
    })
    .map((agent) => agent.id);
}

/**
 * Copies a program over the installed one. A program that is running (an agent's MCP server, the
 * recorder app) cannot be overwritten on Windows, but it can be renamed: it is set aside and the
 * new one copied, so open agents keep working and new runs use the new version. Returns whether
 * one was set aside.
 */
function replace(source, target) {
  try {
    fs.copyFileSync(source, target);
    return false;
  } catch (error) {
    if (error.code !== 'EBUSY' && error.code !== 'EPERM') throw error;
  }
  const aside = `${target}.old-${Date.now()}`;
  try {
    fs.renameSync(target, aside);
  } catch (_) {
    throw new Error(`${target} is in use and could not be replaced: close your agents and Desktop Recorder, then run the installer again`);
  }
  fs.copyFileSync(source, target);
  return true;
}

/** Deletes programs set aside by an earlier install, once nothing runs them any more. */
function removeSetAside(bin) {
  for (const name of fs.existsSync(bin) ? fs.readdirSync(bin) : []) {
    if (/\.exe\.old-\d+$/.test(name)) {
      try {
        fs.rmSync(path.join(bin, name), { force: true });
      } catch (_) {
        /* still running: removed by a later install */
      }
    }
  }
}

/** Where the Start menu shortcut of the recorder app goes. */
function shortcutPath(env, home) {
  const appData = env.APPDATA || path.join(home, 'AppData', 'Roaming');
  return path.join(appData, 'Microsoft', 'Windows', 'Start Menu', 'Programs', 'Desktop Recorder.lnk');
}

/** A Start menu entry for the recorder app, made by Windows' own shell object. */
function addShortcut(where, options) {
  if (process.platform !== 'win32' || !fs.existsSync(where.app)) return null;
  const link = shortcutPath(options.env, options.home);
  fs.mkdirSync(path.dirname(link), { recursive: true });
  const quote = (text) => `'${String(text).replace(/'/g, "''")}'`;
  const script = `$s = (New-Object -ComObject WScript.Shell).CreateShortcut(${quote(link)}); ` +
    `$s.TargetPath = ${quote(where.app)}; $s.WorkingDirectory = ${quote(path.dirname(where.app))}; ` +
    `$s.Description = 'Record a task and play it again, without AI'; $s.Save()`;
  const system32 = path.join(process.env.SystemRoot || process.env.windir || 'C:\\Windows', 'System32');
  execFileSync(path.join(system32, 'WindowsPowerShell', 'v1.0', 'powershell.exe'), ['-NoProfile', '-NonInteractive', '-Command', script], { stdio: 'ignore', windowsHide: true });
  return link;
}

/**
 * agent-desktop (the engine) keeps its state (snapshots, refs) only in a folder no one else can open, and refuses
 * one it finds open to others: it never widens or repairs one it did not make. Other tools may grant
 * themselves access to folders under the person's home (the Codex sandbox gives its own group
 * access to ~/.agent-record-adapter), and a folder made here would inherit that. So the state folder
 * stops inheriting and is opened only to the person, SYSTEM and Administrators, the same three
 * agent-desktop sets on the folders it makes; what is already inside goes back to inheriting from it.
 */
function makePrivate(dir) {
  if (process.platform !== 'win32') {
    fs.chmodSync(dir, 0o700);
    return;
  }
  // Windows' own programs by full path: another whoami (Git's, for one) may come first on PATH.
  const system32 = path.join(process.env.SystemRoot || process.env.windir || 'C:\\Windows', 'System32');
  const user = execFileSync(path.join(system32, 'whoami.exe'), ['/user', '/fo', 'csv', '/nh'], { encoding: 'utf8', windowsHide: true })
    .trim().split(',').pop().replace(/"/g, '');
  // S-1-5-21-... for local, domain and Microsoft accounts; S-1-12-1-... for Microsoft Entra ID accounts.
  if (!/^S-1-\d+(-\d+)+$/.test(user)) throw new Error(`could not read your account's id to protect ${dir}`);
  const icacls = (...args) => execFileSync(path.join(system32, 'icacls.exe'), args, { stdio: 'ignore', windowsHide: true });
  icacls(dir, '/inheritance:r', '/grant:r', `*${user}:(OI)(CI)F`, '*S-1-5-18:(OI)(CI)F', '*S-1-5-32-544:(OI)(CI)F');
  if (fs.readdirSync(dir).length) icacls(path.join(dir, '*'), '/reset', '/T', '/C', '/Q');
}

function version(exe) {
  try {
    const out = execFileSync(exe, ['version'], { encoding: 'utf8', windowsHide: true });
    return JSON.parse(out).data.version;
  } catch (_) {
    return null;
  }
}

function copyDir(from, to) {
  fs.mkdirSync(to, { recursive: true });
  for (const entry of fs.readdirSync(from, { withFileTypes: true })) {
    const a = path.join(from, entry.name);
    const b = path.join(to, entry.name);
    if (entry.isDirectory()) copyDir(a, b);
    else fs.copyFileSync(a, b);
  }
}

function installSkills(agent) {
  if (!agent.skillsDir) return [];
  const installed = [];
  for (const skill of SKILLS) {
    const from = path.join(skillsDir(), skill);
    if (!fs.existsSync(from)) continue;
    const to = path.join(agent.skillsDir, skill);
    fs.rmSync(to, { recursive: true, force: true });
    copyDir(from, to);
    installed.push(skill);
  }
  return installed;
}

function removeSkills(agent) {
  if (!agent.skillsDir) return [];
  return SKILLS.filter((skill) => {
    const dir = path.join(agent.skillsDir, skill);
    if (!fs.existsSync(path.join(dir, MARKER))) return false;
    fs.rmSync(dir, { recursive: true, force: true });
    return true;
  });
}

// Claude Code and Codex rewrite their own configuration while they run: their CLIs change it safely.
// On Windows those CLIs are npm's .cmd scripts, which only the command shell can run, so each call is
// one command line built here, every part quoted (see commandLine). A value that cannot be passed that
// way falls back to editing the configuration file, as for the other agents.
function cliArgs(agent, spec) {
  const env = Object.entries(spec.env).flatMap(([key, value]) => ['--env', `${key}=${value}`]);
  if (agent.id === 'claude-code') {
    return {
      remove: ['claude', 'mcp', 'remove', SERVER, '--scope', 'user'],
      add: ['claude', 'mcp', 'add', SERVER, '--scope', 'user', ...env, '--', spec.command, ...spec.args],
    };
  }
  return {
    remove: ['codex', 'mcp', 'remove', SERVER],
    add: ['codex', 'mcp', 'add', SERVER, ...env, '--', spec.command, ...spec.args],
  };
}

function usesCli(agent, options) {
  return options.useCli && agent.cli && ['claude-code', 'codex'].includes(agent.id) && onPath(agent.cli, options.env);
}

function addServer(agent, where, options) {
  const spec = server(where);
  if (usesCli(agent, options)) {
    const calls = cliArgs(agent, spec);
    const add = commandLine(calls.add);
    if (add) {
      try { run(commandLine(calls.remove), options); } catch (_) { /* not there yet */ }
      run(add, options);
      return;
    }
  }
  config.add(agent.mcp, SERVER, spec);
}

function removeServer(agent, options) {
  if (usesCli(agent, options)) {
    try { run(commandLine(cliArgs(agent, server(layout(options.home))).remove), options); return true; } catch (_) { return false; }
  }
  return config.remove(agent.mcp, SERVER);
}

/**
 * One command line for the command shell, or null when a part cannot be passed through it safely.
 * Plain parts go as they are; the rest in double quotes, inside which the shell takes &, |, <, >, (, )
 * and spaces literally. A part with ", %, ! or ^ is refused: the shell would read those even in quotes.
 */
function commandLine(parts) {
  const quoted = [];
  for (const part of parts) {
    const text = String(part);
    if (/^[A-Za-z0-9_.:\\/=@+,-]+$/.test(text)) quoted.push(text);
    else if (/["%!^\r\n]/.test(text) || text.endsWith('\\')) return null;
    else quoted.push(`"${text}"`);
  }
  return quoted.join(' ');
}

function run(line, options) {
  execSync(line, { stdio: 'ignore', windowsHide: true, env: options.env });
}

/** Installs the program and sets up each chosen agent. Returns one line of result per agent. */
function install(chosen, options) {
  const where = layout(options.home);
  const log = options.log || console.log;
  if (!options.dryRun) {
    installProgram(where, log);
    if (options.installBrowser) {
      try {
        installBrowser(where, options, log);
      } catch (error) {
        log(`agent-browser: not installed (${error.message}); web pages need it: npm install --prefix "${where.browser}" agent-browser`);
      }
    }
    try {
      const link = addShortcut(where, options);
      if (link) log(`Desktop Recorder: Start menu > Desktop Recorder (${where.app})`);
    } catch (_) {
      log(`Desktop Recorder: ${where.app} (no Start menu entry could be made)`);
    }
  }
  const results = [];
  for (const agent of agents(options.home, options.env).filter((a) => chosen.includes(a.id))) {
    if (options.dryRun) {
      results.push(`${agent.name}: would add the MCP server to ${agent.mcp.file}` +
        (agent.skillsDir ? ` and the skills to ${agent.skillsDir}` : ''));
      continue;
    }
    try {
      addServer(agent, where, options);
      const skills = installSkills(agent);
      for (const skill of skills) fs.writeFileSync(path.join(agent.skillsDir, skill, MARKER), '');
      results.push(`${agent.name}: MCP server added` + (skills.length ? `, skills in ${agent.skillsDir}` :
        ' (it reads the guides through the MCP server)'));
    } catch (error) {
      results.push(`${agent.name}: not set up: ${error.message}`);
    }
  }
  return results;
}

function uninstall(chosen, options) {
  const results = [];
  for (const agent of agents(options.home, options.env).filter((a) => chosen.includes(a.id))) {
    try {
      const removed = removeServer(agent, options);
      const skills = removeSkills(agent);
      results.push(`${agent.name}: ${removed ? 'MCP server removed' : 'no MCP server'}` +
        (skills.length ? `, skills removed` : ''));
    } catch (error) {
      results.push(`${agent.name}: ${error.message}`);
    }
  }
  if (options.removeProgram) {
    fs.rmSync(layout(options.home).bin, { recursive: true, force: true });
    fs.rmSync(layout(options.home).browser, { recursive: true, force: true });
    fs.rmSync(shortcutPath(options.env, options.home), { force: true });
    results.push('Program removed (recordings kept in ' + layout(options.home).state + ')');
  }
  return results;
}

function status(options) {
  const where = layout(options.home);
  const browser = agentBrowserPath(where);
  const lines = [
    `Program: ${fs.existsSync(where.exe) ? `${where.exe} (${version(where.exe) || 'unknown version'})` : 'not installed'}`,
    `Engine: ${fs.existsSync(where.engine) ? `${where.engine} (agent-desktop ${version(where.engine) || 'unknown version'})` : 'not installed'}`,
    `agent-browser: ${browser || 'not installed'}`,
  ];
  for (const agent of agents(options.home, options.env)) {
    const found = agent.detect();
    const mcp = config.has(agent.mcp, SERVER);
    const skills = agent.skillsDir && fs.existsSync(path.join(agent.skillsDir, 'agent-record-background', MARKER));
    lines.push(`${agent.name}: ${found ? 'found' : 'not found'}` +
      (found ? `, MCP ${mcp ? 'set up' : 'not set up'}` + (agent.skillsDir ? `, skills ${skills ? 'installed' : 'not installed'}` : '') : ''));
  }
  return lines;
}

module.exports = {
  agentBrowserPath, browserInstallCommand, commandLine, install, installedAgents, uninstall, status, layout, server,
  shortcutPath, MARKER, OFFICIAL_SKILLS, OUR_SKILLS, SERVER, SKILLS,
};
