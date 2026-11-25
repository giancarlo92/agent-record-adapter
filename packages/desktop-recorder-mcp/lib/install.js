// Part of Agent Record Adapter (see NOTICE).
'use strict';

// Installing desktop-recorder-mcp: two programs come inside this one package and go into one
// folder of its own: agent-record (which carries inside the agent-desktop engine with our adapter
// and agent-browser, both built from their unmodified upstream sources) and Desktop Recorder.
// Nothing is downloaded or installed from another package. Then, for each chosen
// agent, the MCP server and, where the agent reads skills from a folder, the skills.

const fs = require('fs');
const path = require('path');
const { execFileSync, execSync } = require('child_process');

const config = require('./config');
const { agents, onPath } = require('./agents');

// The MCP server's name in every agent: the same as the earlier releases of this package, so an
// install replaces them in place.
const SERVER = 'desktop-recorder';
// Names an earlier build of this project used, removed from every agent on install and uninstall.
const LEGACY_SERVERS = ['agent-record'];
const PACKAGE = path.join(__dirname, '..');
// The official skills (copied unmodified from the upstream projects) and ours, which complement them.
const OFFICIAL_SKILLS = ['agent-desktop', 'agent-desktop-windows', 'agent-browser'];
const OUR_SKILLS = ['agent-record-background', 'desktop-recording', 'recording-to-script', 'script-repair'];
// The install folder's name and the mark left in each skill folder we copy: the same as the earlier
// releases, so their skills, saved passwords and recordings carry over.
const MARKER = '.desktop-recorder-mcp';
// What earlier builds left that this one no longer uses: agent-browser installed from npm, and the
// engine and agent-browser as programs of their own (agent-record carries both inside now).
const LEGACY_FOLDERS = ['browser'];
const LEGACY_PROGRAMS = ['agent-desktop.exe', 'agent-browser.exe'];

/** The built programs: the package's vendor folder, or DESKTOP_RECORDER_MCP_VENDOR (tests). */
function vendorDir() {
  return process.env.DESKTOP_RECORDER_MCP_VENDOR || path.join(PACKAGE, 'vendor');
}

/** The skills folder: the package's, or DESKTOP_RECORDER_MCP_SKILLS (tests and a repository checkout). */
function skillsDir() {
  return process.env.DESKTOP_RECORDER_MCP_SKILLS || path.join(PACKAGE, 'skills');
}
const SKILLS = [...OFFICIAL_SKILLS, ...OUR_SKILLS];

/** Where everything goes for one home folder. */
function layout(home) {
  const root = path.join(home, MARKER);
  return {
    root,
    bin: path.join(root, 'bin'),
    // The one program every agent and script runs: agent-desktop's commands plus ours, web pages
    // (agent-browser) and the MCP server; it carries the engine and agent-browser inside.
    exe: path.join(root, 'bin', 'agent-record.exe'),
    // The small recorder app (Record / Stop, its recordings, Play), next to agent-record.
    app: path.join(root, 'bin', 'desktop-recorder.exe'),
    // The tool's private state (refs, snapshots, recordings), apart from the upstream tool's ~/.agent-desktop.
    state: path.join(root, 'state'),
  };
}

function server(where) {
  return { command: where.exe, args: ['mcp'], env: { AGENT_DESKTOP_HOME: where.state } };
}

/** The programs the package carries, by file name, and where each one goes. */
function programs(where) {
  return [
    ['agent-record.exe', where.exe],
    ['desktop-recorder.exe', where.app],
  ];
}

function installProgram(where, log) {
  const vendor = vendorDir();
  for (const [name] of programs(where)) {
    const file = path.join(vendor, name);
    if (!fs.existsSync(file)) throw new Error(`The package has no program (${file}); run npm run prepare-vendor`);
  }
  fs.mkdirSync(where.bin, { recursive: true });
  fs.mkdirSync(where.state, { recursive: true });
  makePrivate(where.state);
  makeWritableOnlyByYou(where.bin);
  removeSetAside(where.bin);
  for (const folder of LEGACY_FOLDERS) fs.rmSync(path.join(where.root, folder), { recursive: true, force: true });
  const kept = programs(where).map(([name, target]) => replace(path.join(vendor, name), target)).some(Boolean);
  const retired = LEGACY_PROGRAMS.map((name) => retire(path.join(where.bin, name))).some(Boolean);
  // The recorder app reads it to know which version it is, and whether a newer one is out.
  fs.writeFileSync(path.join(where.bin, 'version.txt'), require('../package.json').version);
  log(`Program: ${where.exe}`);
  if (kept || retired) log('Agents that were open keep the previous version until they restart.');
}

/**
 * Removes a program an earlier build installed and this one carries inside agent-record. One that
 * is running is set aside instead (renamed), and a later install deletes it. Returns whether it
 * was set aside.
 */
function retire(file) {
  if (!fs.existsSync(file)) return false;
  try {
    fs.rmSync(file, { force: true });
    return false;
  } catch (error) {
    if (error.code !== 'EBUSY' && error.code !== 'EPERM') throw error;
  }
  try {
    fs.renameSync(file, `${file}.old-${Date.now()}`);
    return true;
  } catch (_) {
    return false;
  }
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
 * access to ~/.desktop-recorder-mcp), and a folder made here would inherit that. So the state folder
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

/**
 * The programs folder can be changed only by the person, SYSTEM and Administrators; everyone else
 * (another account, a sandbox group such as Codex's, which gives itself access to folders under the
 * person's home) can read and run what is in it but not replace it. Otherwise another program
 * could swap agent-record.exe for its own and every agent would run it.
 */
function makeWritableOnlyByYou(dir) {
  if (process.platform !== 'win32') {
    fs.chmodSync(dir, 0o755);
    return;
  }
  const system32 = path.join(process.env.SystemRoot || process.env.windir || 'C:\\Windows', 'System32');
  const user = execFileSync(path.join(system32, 'whoami.exe'), ['/user', '/fo', 'csv', '/nh'], { encoding: 'utf8', windowsHide: true })
    .trim().split(',').pop().replace(/"/g, '');
  if (!/^S-1-\d+(-\d+)+$/.test(user)) throw new Error(`could not read your account's id to protect ${dir}`);
  const icacls = (...args) => execFileSync(path.join(system32, 'icacls.exe'), args, { stdio: 'ignore', windowsHide: true });
  // S-1-5-11: Authenticated Users, read and run only.
  icacls(dir, '/inheritance:r', '/grant:r', `*${user}:(OI)(CI)F`, '*S-1-5-18:(OI)(CI)F', '*S-1-5-32-544:(OI)(CI)F',
    '*S-1-5-11:(OI)(CI)RX');
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
function cliArgs(agent, spec, name = SERVER) {
  const env = Object.entries(spec.env).flatMap(([key, value]) => ['--env', `${key}=${value}`]);
  if (agent.id === 'claude-code') {
    return {
      remove: ['claude', 'mcp', 'remove', name, '--scope', 'user'],
      add: ['claude', 'mcp', 'add', name, '--scope', 'user', ...env, '--', spec.command, ...spec.args],
    };
  }
  return {
    remove: ['codex', 'mcp', 'remove', name],
    add: ['codex', 'mcp', 'add', name, ...env, '--', spec.command, ...spec.args],
  };
}

function usesCli(agent, options) {
  return options.useCli && agent.cli && ['claude-code', 'codex'].includes(agent.id) && onPath(agent.cli, options.env);
}

/** Removes one server by name, through the agent's CLI when it has one. Returns whether it was there. */
function removeNamed(agent, name, options) {
  if (!config.has(agent.mcp, name)) return false;
  if (usesCli(agent, options)) {
    const line = commandLine(cliArgs(agent, server(layout(options.home)), name).remove);
    try {
      if (line) {
        run(line, options);
        return true;
      }
    } catch (_) { /* fall back to the file */ }
  }
  return config.remove(agent.mcp, name);
}

/** Removes the names earlier builds used, so an agent never lists two of these servers. */
function removeLegacy(agent, options) {
  return LEGACY_SERVERS.filter((name) => removeNamed(agent, name, options));
}

function addServer(agent, where, options) {
  removeLegacy(agent, options);
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
  const legacy = removeLegacy(agent, options);
  return removeNamed(agent, SERVER, options) || legacy.length > 0;
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
    if (options.dryRun) {
      let set = false;
      try {
        set = config.has(agent.mcp, SERVER);
      } catch (_) { /* unreadable: nothing to say it has */ }
      results.push(`${agent.name}: ${set ? 'would remove the MCP server and its skills' : 'no MCP server'}`);
      continue;
    }
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
    const where = layout(options.home);
    if (options.dryRun) {
      results.push(`Would remove the programs in ${where.bin} and the Start menu shortcut (recordings kept in ${where.state})`);
      return results;
    }
    const busy = removeFolder(where.bin);
    for (const folder of LEGACY_FOLDERS) removeFolder(path.join(where.root, folder));
    fs.rmSync(shortcutPath(options.env, options.home), { force: true });
    results.push(busy.length
      ? `Program removed, except ${busy.join(', ')}: still running (an agent or Desktop Recorder is open), so renamed aside; close them and run uninstall again, or the next install deletes them (recordings kept in ${where.state})`
      : `Program removed (recordings kept in ${where.state})`);
  }
  return results;
}

/** The name Windows' Credential Manager gives a saved password of this program (keyring's target). */
const CREDENTIAL_SUFFIX = '.desktop-recorder-mcp';

/**
 * Every folder this program leaves on the computer, with what it holds: the programs and their
 * state, the dedicated browsers' profiles, recordings kept in the app's own folder, agent-desktop's
 * and agent-browser's state (both run inside agent-record; kept when the official programs are
 * installed too) and npx's copy of this package. A recordings folder the person chose is not one.
 */
function leftovers(options) {
  const { home, env } = options;
  const where = layout(home);
  const local = env.LOCALAPPDATA || path.join(home, 'AppData', 'Local');
  const folders = [
    { path: where.root, what: 'the programs, their state, the names of saved passwords and the app settings' },
    { path: path.join(local, 'DesktopRecorder'), what: "the dedicated browsers' profiles and temporary files" },
    { path: path.join(local, 'AgentRecordAdapter'), what: 'files of an earlier version' },
    { path: defaultRecordings(home), what: "recordings saved in Desktop Recorder's own folder" },
  ];
  if (!onPath('agent-desktop', env)) folders.push({ path: path.join(home, '.agent-desktop'), what: "the engine's state" });
  if (!onPath('agent-browser', env)) folders.push({ path: path.join(home, '.agent-browser'), what: "agent-browser's state" });
  for (const dir of npxCopies(local)) folders.push({ path: dir, what: "npx's copy of this package" });
  return folders.filter((folder) => fs.existsSync(folder.path));
}

/** Where Desktop Recorder saves recordings unless the person picks another folder. */
function defaultRecordings(home) {
  return path.join(home, 'Documents', 'Desktop Recorder');
}

/** The recordings folder the person picked in Desktop Recorder, when it is not the app's own. */
function chosenRecordings(home) {
  try {
    const saved = JSON.parse(fs.readFileSync(path.join(layout(home).root, 'recorder.json'), 'utf8'));
    const folder = saved.folder && path.resolve(saved.folder);
    return folder && folder.toLowerCase() !== path.resolve(defaultRecordings(home)).toLowerCase() ? folder : null;
  } catch (_) {
    return null;
  }
}

/** npx's folders that hold only this package (npm-cache\_npx\<hash>). */
function npxCopies(local) {
  const base = path.join(local, 'npm-cache', '_npx');
  if (!fs.existsSync(base)) return [];
  return fs.readdirSync(base)
    .map((name) => path.join(base, name))
    .filter((dir) => {
      try {
        const deps = Object.keys(JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8')).dependencies || {});
        return deps.length === 1 && deps[0] === 'desktop-recorder-mcp';
      } catch (_) {
        return false;
      }
    });
}

/** The saved passwords in Windows' Credential Manager (their targets, never their values). */
function savedPasswords(options) {
  if (process.platform !== 'win32' || options.home !== require('os').homedir()) return [];
  let text = '';
  try {
    text = execFileSync('cmdkey', ['/list'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  } catch (_) {
    return [];
  }
  const targets = new Set();
  for (const line of text.split(/\r?\n/)) {
    const match = line.match(/([^\s=:]+\.desktop-recorder-mcp)\s*$/);
    if (match && match[1].endsWith(CREDENTIAL_SUFFIX)) targets.add(match[1]);
  }
  return [...targets];
}

/**
 * The programs of this package still running: agent-record and Desktop Recorder from its folder
 * (an agent's MCP server, the DevTools proxy, a recording) and the browsers started with one of its
 * profiles. Never this process or the ones that started it.
 */
function runningPrograms(options) {
  if (process.platform !== 'win32' || options.home !== require('os').homedir()) return [];
  const where = layout(options.home);
  const local = options.env.LOCALAPPDATA || path.join(options.home, 'AppData', 'Local');
  const bin = where.bin.toLowerCase();
  const profiles = [path.join(local, 'DesktopRecorder'), path.join(local, 'AgentRecordAdapter'), path.join(where.root, 'script-browser')]
    .map((dir) => dir.toLowerCase());
  let list = [];
  try {
    const json = execFileSync('powershell', ['-NoProfile', '-Command',
      'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress'],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 64 * 1024 * 1024 });
    list = [].concat(JSON.parse(json) || []);
  } catch (_) {
    return [];
  }
  const own = new Set([process.pid, process.ppid]);
  return list.filter((p) => {
    if (own.has(p.ProcessId)) return false;
    const exe = (p.ExecutablePath || '').toLowerCase();
    const line = (p.CommandLine || '').toLowerCase();
    return exe.startsWith(bin + path.sep) || profiles.some((dir) => line.includes(dir));
  }).map((p) => ({ pid: p.ProcessId, name: path.basename(p.ExecutablePath || 'program') }));
}

/** What a complete uninstall removes, said before it does. */
function plan(options) {
  const lines = [];
  const running = runningPrograms(options);
  if (running.length) lines.push(`Stop ${running.length} program(s) of it still running (${[...new Set(running.map((p) => p.name))].join(', ')})`);
  lines.push("Remove the MCP server and its skills from every agent, the backups of their configuration, and the Start menu entry");
  const passwords = savedPasswords(options);
  if (passwords.length) lines.push(`Delete ${passwords.length} saved password(s) from Windows' Credential Manager`);
  for (const folder of leftovers(options)) lines.push(`Delete ${folder.path}: ${folder.what}`);
  const kept = chosenRecordings(options.home);
  if (kept) lines.push(`Keep your recordings in ${kept} (the folder you chose)`);
  return lines;
}

/**
 * Removes everything this package put on the computer, so nothing of it is left: its programs
 * (stopped first), every agent's MCP server and skills, its saved passwords, its folders and its
 * Start menu entry. Recordings in a folder the person chose stay, and so do scripts they saved
 * elsewhere.
 */
function removeEverything(options) {
  if (options.dryRun) return plan(options).map((line) => `Would: ${line}`);
  const results = [];
  const kept = chosenRecordings(options.home);
  const running = runningPrograms(options);
  for (const program of running) {
    try {
      execFileSync('taskkill', ['/PID', String(program.pid), '/T', '/F'], { stdio: 'ignore' });
    } catch (_) { /* it ended on its own */ }
  }
  if (running.length) results.push(`Stopped ${running.length} program(s) of it`);
  const all = agents(options.home, options.env).filter((agent) => agent.detect()).map((agent) => agent.id);
  results.push(...uninstall(all, { ...options, removeProgram: false }).filter((line) => !/: no MCP server$/.test(line)));
  let deleted = 0;
  for (const target of savedPasswords(options)) {
    try {
      execFileSync('cmdkey', [`/delete:${target}`], { stdio: 'ignore' });
      deleted += 1;
    } catch (_) { /* already gone */ }
  }
  if (deleted) results.push(`Deleted ${deleted} saved password(s)`);
  // The backup each agent's configuration got when the server was added: the configuration itself
  // no longer has the server.
  for (const agent of agents(options.home, options.env)) {
    const backup = agent.mcp && agent.mcp.file && `${agent.mcp.file}${config.BACKUP_SUFFIX}`;
    if (backup && fs.existsSync(backup)) fs.rmSync(backup, { force: true });
  }
  fs.rmSync(shortcutPath(options.env, options.home), { force: true });
  const stuck = [];
  for (const folder of leftovers(options)) {
    const busy = removeFolder(folder.path);
    if (busy.length || fs.existsSync(folder.path)) stuck.push(folder.path);
    else results.push(`Deleted ${folder.path}`);
  }
  if (stuck.length) {
    results.push(`Could not delete ${stuck.join(', ')}: something still uses it. Close your agents and run uninstall again`);
  }
  if (kept) results.push(`Kept your recordings in ${kept}`);
  results.push(stuck.length ? 'Uninstalled, apart from what is listed above' : 'Uninstalled: nothing of it is left on this computer');
  return results;
}

/**
 * Deletes a folder. A program still running in it cannot be deleted on Windows: it is renamed aside
 * (`.old-<time>`, as an install does) and its name returned; the folder then stays with those files.
 */
function removeFolder(dir) {
  if (!fs.existsSync(dir)) return [];
  try {
    fs.rmSync(dir, { recursive: true, force: true });
    return [];
  } catch (error) {
    if (error.code !== 'EBUSY' && error.code !== 'EPERM') throw error;
  }
  const busy = [];
  for (const name of fs.readdirSync(dir)) {
    const file = path.join(dir, name);
    if (fs.statSync(file).isDirectory()) {
      busy.push(...removeFolder(file));
    } else if (!/\.old-\d+$/.test(name) && retire(file)) {
      busy.push(name);
    } else if (fs.existsSync(file)) {
      try {
        fs.rmSync(file, { force: true });
      } catch (_) {
        busy.push(name);
      }
    }
  }
  try {
    fs.rmdirSync(dir);
  } catch (_) { /* files still running are in it */ }
  return busy;
}

function status(options) {
  const where = layout(options.home);
  const lines = [
    `Program: ${fs.existsSync(where.exe) ? `${where.exe} (${version(where.exe) || 'unknown version'})` : 'not installed'}`,
    `Engine and agent-browser: inside agent-record${fs.existsSync(where.exe) ? ` (agent-desktop ${version(where.exe) || 'unknown version'})` : ''}`,
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
  commandLine, install, installedAgents, uninstall, removeEverything, plan, leftovers, status, layout, server,
  shortcutPath, LEGACY_SERVERS, MARKER, OFFICIAL_SKILLS, OUR_SKILLS, SERVER, SKILLS,
};
