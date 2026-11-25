// Part of Agent Record Adapter (see NOTICE).
'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');

const { install, uninstall, status, layout, SERVER, OFFICIAL_SKILLS, OUR_SKILLS } = require('../lib/install');
const config = require('../lib/config');

// The programs and skills the installer copies, made once in a temporary folder: a release build is
// not needed to test the installer. On Windows each program is a copy of findstr.exe, a real program
// that waits on its input like an MCP server does (`agent-record.exe mcp` reads until its input ends).
const repo = path.join(__dirname, '..', '..', '..');
const fixtures = fs.mkdtempSync(path.join(os.tmpdir(), 'ara-package-'));
const vendor = path.join(fixtures, 'vendor');
fs.mkdirSync(vendor);
const program = process.platform === 'win32'
  ? path.join(process.env.SystemRoot || 'C:\\Windows','System32', 'findstr.exe')
  : null;
for (const name of ['agent-record.exe', 'desktop-recorder.exe']) {
  if (program) fs.copyFileSync(program, path.join(vendor, name));
  else fs.writeFileSync(path.join(vendor, name), name);
}
const skills = path.join(fixtures, 'skills');
const official = {
  'agent-desktop': path.join(repo, 'vendor', 'agent-desktop', 'skills', 'agent-desktop'),
  'agent-desktop-windows': path.join(repo, 'vendor', 'agent-desktop', 'skills', 'agent-desktop-windows'),
  'agent-browser': path.join(repo, 'vendor', 'agent-browser', 'skills', 'agent-browser'),
};
for (const skill of [...OFFICIAL_SKILLS, ...OUR_SKILLS]) {
  fs.cpSync(official[skill] || path.join(repo, 'skills', skill), path.join(skills, skill), { recursive: true });
}
process.env.DESKTOP_RECORDER_MCP_VENDOR = vendor;
process.env.DESKTOP_RECORDER_MCP_SKILLS = skills;

function fakeHome() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'adr-home-'));
  const env = { APPDATA: path.join(home, 'AppData', 'Roaming'), LOCALAPPDATA: path.join(home, 'AppData', 'Local'), PATH: '' };
  for (const dir of ['.claude', '.codex', '.cursor', path.join('AppData', 'Roaming', 'Code', 'User')]) {
    fs.mkdirSync(path.join(home, dir), { recursive: true });
  }
  fs.writeFileSync(path.join(home, '.claude.json'), JSON.stringify({ theme: 'dark', mcpServers: { other: { command: 'x' } } }));
  fs.writeFileSync(path.join(home, '.codex', 'config.toml'), 'model = "gpt"\n\n[mcp_servers.other]\ncommand = "x"\n');
  return { home, env, options: { home, env, useCli: false, log: () => {} } };
}

const read = (file) => fs.readFileSync(file, 'utf8');

test('install sets up each chosen agent and keeps what was there', () => {
  const { home, options } = fakeHome();
  const lines = install(['claude-code', 'codex', 'cursor', 'vscode'], options);
  const where = layout(home);
  assert.ok(fs.existsSync(where.exe), 'agent-record is copied');
  assert.deepStrictEqual(fs.readdirSync(where.bin).filter((f) => f.endsWith('.exe')).sort(),
    ['agent-record.exe', 'desktop-recorder.exe'], 'only the two programs: the engine and agent-browser are inside agent-record');

  const claude = JSON.parse(read(path.join(home, '.claude.json')));
  assert.equal(claude.theme, 'dark');
  assert.ok(claude.mcpServers.other, 'another server is kept');
  assert.equal(claude.mcpServers[SERVER].command, where.exe);
  assert.deepEqual(claude.mcpServers[SERVER].args, ['mcp']);
  assert.equal(claude.mcpServers[SERVER].env.AGENT_DESKTOP_HOME, where.state);
  assert.ok(fs.existsSync(path.join(home, '.claude.json' + config.BACKUP_SUFFIX)), 'a backup is kept');

  const toml = read(path.join(home, '.codex', 'config.toml'));
  assert.match(toml, /model = "gpt"/);
  assert.match(toml, /\[mcp_servers\.other\]/);
  assert.match(toml, new RegExp(`\\[mcp_servers\\.${SERVER}\\]`));
  assert.ok(toml.includes(`command = '${where.exe}'`), 'a Windows path is a literal TOML string');

  const vscode = JSON.parse(read(path.join(home, 'AppData', 'Roaming', 'Code', 'User', 'mcp.json')));
  assert.equal(vscode.servers[SERVER].type, 'stdio');
  assert.ok(JSON.parse(read(path.join(home, '.cursor', 'mcp.json'))).mcpServers[SERVER]);

  assert.ok(fs.existsSync(path.join(home, '.claude', 'skills', 'agent-desktop', 'SKILL.md')), 'Claude Code gets the skills');
  for (const skill of ['agent-browser', 'agent-record-background', 'desktop-recording', 'recording-to-script', 'script-repair']) {
    assert.ok(fs.existsSync(path.join(home, '.claude', 'skills', skill, 'SKILL.md')), `Claude Code gets ${skill}`);
    assert.ok(fs.existsSync(path.join(home, '.codex', 'skills', skill, 'SKILL.md')), `Codex gets ${skill}`);
  }
  assert.ok(fs.existsSync(path.join(home, '.claude', 'skills', 'recording-to-script', 'scripts', 'desktop-script.mjs')),
    'the script helpers come with the skill');
  assert.ok(fs.existsSync(path.join(home, '.codex', 'skills', 'agent-desktop-windows', 'SKILL.md')), 'Codex gets the skills');
  assert.equal(lines.length, 4);
  assert.ok(lines.every((line) => !line.includes('not set up')), lines.join('\n'));
});

test('the state folder opens only to the person, SYSTEM and Administrators, whatever its parent grants', { skip: process.platform !== 'win32' }, () => {
  const { execFileSync } = require('child_process');
  const { home, options } = fakeHome();
  const where = layout(home);
  // What the Codex sandbox does to ~/.desktop-recorder-mcp: a group of its own, inherited by what is inside.
  fs.mkdirSync(path.join(where.state, 'snapshots'), { recursive: true });
  fs.writeFileSync(path.join(where.state, 'snapshots', 'old'), 'x');
  execFileSync('icacls', [where.root, '/grant', '*S-1-5-32-545:(OI)(CI)(M)'], { stdio: 'ignore' });
  install(['claude-code'], options);
  for (const target of [where.state, path.join(where.state, 'snapshots', 'old')]) {
    const acl = execFileSync('icacls', [target], { encoding: 'utf8' });
    assert.ok(!/S-1-5-32-545|BUILTIN\\Users|BUILTIN\\Usuarios/i.test(acl), acl);
  }
  const own = execFileSync('icacls', [where.state], { encoding: 'utf8' });
  assert.ok(!own.includes('(I)'), `the state folder must not inherit: ${own}`);
});

test('the recorder app is installed next to agent-record, with a Start menu entry that uninstall removes', { skip: process.platform !== 'win32' }, () => {
  const { home, env, options } = fakeHome();
  const { shortcutPath } = require('../lib/install');
  install(['claude-code'], options);
  const where = layout(home);
  assert.ok(fs.existsSync(where.app), 'desktop-recorder.exe is copied');
  const link = shortcutPath(env, home);
  assert.ok(fs.existsSync(link), `the shortcut is made: ${link}`);
  uninstall(['claude-code'], { ...options, removeProgram: true });
  assert.ok(!fs.existsSync(where.app));
  assert.ok(!fs.existsSync(link), 'the shortcut is removed with the program');
});

test('installing twice leaves one server, and uninstall removes only ours', () => {
  const { home, options } = fakeHome();
  install(['claude-code', 'codex'], options);
  install(['claude-code', 'codex'], options);
  const toml = read(path.join(home, '.codex', 'config.toml'));
  assert.equal(toml.split(`[mcp_servers.${SERVER}]`).length - 1, 1, 'one block');

  uninstall(['claude-code', 'codex'], options);
  const claude = JSON.parse(read(path.join(home, '.claude.json')));
  assert.ok(!claude.mcpServers[SERVER]);
  assert.ok(claude.mcpServers.other);
  const after = read(path.join(home, '.codex', 'config.toml'));
  assert.ok(!after.includes(SERVER), after);
  assert.match(after, /\[mcp_servers\.other\]/);
  assert.ok(!fs.existsSync(path.join(home, '.claude', 'skills', 'agent-desktop')), 'our skills are removed');
});

test('a skill folder the installer did not write is never removed', () => {
  const { home, options } = fakeHome();
  const own = path.join(home, '.claude', 'skills', 'agent-desktop');
  fs.mkdirSync(own, { recursive: true });
  fs.writeFileSync(path.join(own, 'SKILL.md'), 'the person installed the official skill');
  uninstall(['claude-code'], options);
  assert.ok(fs.existsSync(path.join(own, 'SKILL.md')));
});

test('a JSON file with comments is reported and left as it was', () => {
  const { home, options } = fakeHome();
  const file = path.join(home, '.cursor', 'mcp.json');
  fs.writeFileSync(file, '{\n  // mine\n  "mcpServers": {}\n}\n');
  const [line] = install(['cursor'], options);
  assert.match(line, /not set up/);
  assert.equal(read(file), '{\n  // mine\n  "mcpServers": {}\n}\n');
});

test('a dry run changes nothing', () => {
  const { home, options } = fakeHome();
  const before = read(path.join(home, '.claude.json'));
  const lines = install(['claude-code'], { ...options, dryRun: true });
  assert.match(lines[0], /would add/);
  assert.equal(read(path.join(home, '.claude.json')), before);
  assert.ok(!fs.existsSync(layout(home).exe));
});

test('status says what is set up', () => {
  const { options } = fakeHome();
  install(['claude-code'], options);
  const lines = status(options);
  assert.ok(lines.some((l) => /^Claude Code: found, MCP set up, skills installed/.test(l)), lines.join('\n'));
  assert.ok(lines.some((l) => /^Codex \(the codex command and the Codex app\): found, MCP not set up/.test(l)), lines.join('\n'));
});

test('a command line quotes what needs it and refuses what the shell would still read', () => {
  const { commandLine } = require('../lib/install');
  assert.strictEqual(
    commandLine(['claude', 'mcp', 'add', 'x', '--env', 'K=C:\\Users\\Ana\\.tool', '--', 'C:\\Program Files (x86)\\a & b\\t.exe', 'mcp']),
    'claude mcp add x --env K=C:\\Users\\Ana\\.tool -- "C:\\Program Files (x86)\\a & b\\t.exe" mcp'
  );
  for (const unsafe of ['a"b', '%PATH%', 'wow!', 'x^y', 'line\nbreak', 'C:\\dir with space\\']) {
    assert.strictEqual(commandLine(['claude', unsafe]), null, unsafe);
  }
});

test('the agents CLIs get every argument intact, through npm .cmd scripts, without a Node warning', { skip: process.platform !== 'win32' }, () => {
  const { spawnSync } = require('child_process');
  const { home, env } = fakeHome();
  const bin = path.join(home, 'npm-bin');
  fs.mkdirSync(bin);
  const log = path.join(home, 'calls.jsonl');
  fs.writeFileSync(path.join(bin, 'record.js'), `require('fs').appendFileSync(${JSON.stringify(log)}, JSON.stringify(process.argv.slice(2)) + '\\n');`);
  for (const cli of ['claude', 'codex']) {
    // The shape of a script npm installs for a global command.
    fs.writeFileSync(path.join(bin, `${cli}.cmd`), `@ECHO off\r\n"${process.execPath}" "%~dp0\\record.js" ${cli} %*\r\n`);
  }
  const homeWithSpace = path.join(home, 'Ana & Co (work)');
  fs.mkdirSync(homeWithSpace);
  const script = `
    const { install } = require(${JSON.stringify(path.join(__dirname, '..', 'lib', 'install'))});
    install(['claude-code', 'codex'], { home: ${JSON.stringify(homeWithSpace)}, env: process.env, useCli: true, log: () => {} });`;
  const childEnv = { ...process.env, ...env, PATH: `${bin};${process.env.PATH}` };
  const child = spawnSync(process.execPath, ['-e', script], { env: childEnv, encoding: 'utf8' });
  assert.strictEqual(child.status, 0, child.stderr);
  const stderr = child.stderr;
  const calls = read(log).trim().split('\n').map((line) => JSON.parse(line));
  const where = layout(homeWithSpace);
  assert.deepStrictEqual(calls.find((c) => c[0] === 'claude' && c[2] === 'add'),
    ['claude', 'mcp', 'add', SERVER, '--scope', 'user', '--env', `AGENT_DESKTOP_HOME=${where.state}`,
      '--', where.exe, 'mcp']);
  assert.deepStrictEqual(calls.find((c) => c[0] === 'codex' && c[2] === 'add'),
    ['codex', 'mcp', 'add', SERVER, '--env', `AGENT_DESKTOP_HOME=${where.state}`,
      '--', where.exe, 'mcp']);
  assert.ok(!/DEP0190|DeprecationWarning/.test(stderr || ''), stderr);
});

test('Claude Desktop is set up where recent versions read it (Claude-Data), else where older ones did', () => {
  const { agents } = require('../lib/agents');
  const { home, env } = fakeHome();
  const desktop = () => agents(home, env).find((a) => a.id === 'claude-desktop');
  const old = path.join(env.APPDATA, 'Claude');
  fs.mkdirSync(old, { recursive: true });
  assert.strictEqual(desktop().mcp.file, path.join(old, 'claude_desktop_config.json'));
  const current = path.join(env.LOCALAPPDATA, 'Claude-Data', 'claude_pzs8sxrjxfjjc');
  fs.mkdirSync(current, { recursive: true });
  assert.strictEqual(desktop().mcp.file, path.join(current, 'claude_desktop_config.json'));
  assert.ok(desktop().detect());
});

test('a program that is running is set aside and replaced, so open agents do not block an install', { skip: process.platform !== 'win32' }, async () => {
  const { spawn } = require('child_process');
  const { home, options } = fakeHome();
  install(['claude-code'], options);
  const where = layout(home);
  // Run the installed program as an agent does (an MCP server waiting on its input), so Windows holds its file.
  const running = spawn(where.exe, ['mcp'], { stdio: ['pipe', 'ignore', 'ignore'], env: { ...process.env, AGENT_DESKTOP_HOME: where.state } });
  await new Promise((done) => setTimeout(done, 1500));
  try {
    const logged = [];
    install(['claude-code'], { ...options, log: (line) => logged.push(line) });
    assert.ok(logged.some((line) => line.includes('keep the previous version')), logged.join('\n'));
    assert.ok(fs.readdirSync(where.bin).some((name) => /agent-record\.exe\.old-\d+$/.test(name)), 'the running one is set aside');
    assert.equal(fs.statSync(where.exe).size, fs.statSync(path.join(vendor, 'agent-record.exe')).size);
  } finally {
    running.kill();
    await new Promise((done) => setTimeout(done, 800));
  }
  install(['claude-code'], options);
  assert.ok(!fs.readdirSync(where.bin).some((name) => name.includes('.old-')), 'a later install deletes what was set aside');
});

test('update sets up again only the agents that have it, and writes the version and how it went', { skip: process.platform !== 'win32' }, () => {
  const { execFileSync } = require('child_process');
  const { installedAgents } = require('../lib/install');
  const { home, options } = fakeHome();
  install(['codex'], options);
  assert.deepEqual(installedAgents(options), ['codex']);
  execFileSync(process.execPath, [path.join(__dirname, '..', 'cli.js'), 'update', '--home', home], { stdio: 'ignore' });
  const where = layout(home);
  assert.equal(fs.readFileSync(path.join(where.bin, 'version.txt'), 'utf8'), require('../package.json').version);
  const result = JSON.parse(fs.readFileSync(path.join(where.root, 'update-result.json'), 'utf8'));
  assert.equal(result.ok, true);
  assert.deepEqual(installedAgents(options), ['codex'], 'no other agent was set up');
});

test('the command line with --home keeps the Start menu entry inside that home', { skip: process.platform !== 'win32' }, () => {
  const { execFileSync } = require('child_process');
  const { shortcutPath } = require('../lib/install');
  const { home } = fakeHome();
  const realLink = shortcutPath(process.env, os.homedir());
  const before = fs.existsSync(realLink) ? fs.statSync(realLink).mtimeMs : null;
  execFileSync(process.execPath, [path.join(__dirname, '..', 'cli.js'), 'install', '--home', home, '--agents', 'codex'], { stdio: 'ignore' });
  assert.ok(fs.existsSync(path.join(home, 'AppData', 'Roaming', 'Microsoft', 'Windows', 'Start Menu', 'Programs', 'Desktop Recorder.lnk')));
  const after = fs.existsSync(realLink) ? fs.statSync(realLink).mtimeMs : null;
  assert.equal(after, before, 'the real Start menu entry is not touched');
});

test('the engine and agent-browser an earlier build installed as programs are removed: agent-record carries them', () => {
  const { server } = require('../lib/install');
  const { home, options } = fakeHome();
  const where = layout(home);
  fs.mkdirSync(where.bin, { recursive: true });
  fs.writeFileSync(path.join(where.bin, 'agent-desktop.exe'), 'old engine');
  fs.writeFileSync(path.join(where.bin, 'agent-browser.exe'), 'old browser');
  install(['claude-code'], options);
  assert.ok(!fs.existsSync(path.join(where.bin, 'agent-desktop.exe')), 'the old engine is removed');
  assert.ok(!fs.existsSync(path.join(where.bin, 'agent-browser.exe')), 'the old agent-browser is removed');
  assert.deepEqual(server(where).env, { AGENT_DESKTOP_HOME: where.state });
  const claude = JSON.parse(read(path.join(home, '.claude.json')));
  assert.equal(claude.mcpServers[SERVER].env.AGENT_BROWSER_BIN, undefined);
});

test('nothing is installed from another package: no npm folder, and the one an earlier build made is removed', () => {
  const source = read(path.join(__dirname, '..', 'lib', 'install.js'));
  assert.ok(!/npm install|npx /.test(source), 'the installer runs no npm install');
  const { home, options } = fakeHome();
  const where = layout(home);
  const old = path.join(where.root, 'browser', 'node_modules', 'agent-browser');
  fs.mkdirSync(old, { recursive: true });
  install(['claude-code'], options);
  assert.ok(!fs.existsSync(path.join(where.root, 'browser')), 'the npm folder of an earlier build is gone');
});

test('the server is named desktop-recorder, and the name an earlier build used is removed', () => {
  const { LEGACY_SERVERS } = require('../lib/install');
  assert.strictEqual(SERVER, 'desktop-recorder');
  const { home, options } = fakeHome();
  const file = path.join(home, '.claude.json');
  const data = JSON.parse(read(file));
  for (const name of LEGACY_SERVERS) data.mcpServers[name] = { command: 'old.exe', args: ['mcp'] };
  fs.writeFileSync(file, JSON.stringify(data));
  fs.appendFileSync(path.join(home, '.codex', 'config.toml'), `\n[mcp_servers.${LEGACY_SERVERS[0]}]\ncommand = 'old.exe'\n`);
  install(['claude-code', 'codex'], options);
  const claude = JSON.parse(read(file));
  for (const name of LEGACY_SERVERS) assert.ok(!claude.mcpServers[name], name);
  assert.ok(claude.mcpServers[SERVER] && claude.mcpServers.other);
  const toml = read(path.join(home, '.codex', 'config.toml'));
  assert.ok(!toml.includes(`[mcp_servers.${LEGACY_SERVERS[0]}]`), toml);
});

test('an earlier desktop-recorder install is replaced in place, keeping saved passwords, settings and recordings', () => {
  const { home, options } = fakeHome();
  const where = layout(home);
  assert.strictEqual(where.root, path.join(home, '.desktop-recorder-mcp'));
  fs.mkdirSync(where.bin, { recursive: true });
  fs.mkdirSync(path.join(where.state, 'recordings'), { recursive: true });
  fs.writeFileSync(where.exe, 'old agent-record');
  fs.writeFileSync(path.join(where.root, 'secrets.json'), '[{"name":"x"}]');
  fs.writeFileSync(path.join(where.root, 'recorder.json'), '{"video":true}');
  fs.writeFileSync(path.join(where.state, 'recordings', 'r.json'), '{}');
  const oldSkill = path.join(home, '.claude', 'skills', 'desktop-recording');
  fs.mkdirSync(oldSkill, { recursive: true });
  fs.writeFileSync(path.join(oldSkill, 'SKILL.md'), 'old');
  fs.writeFileSync(path.join(oldSkill, '.desktop-recorder-mcp'), '');
  install(['claude-code'], options);
  assert.notStrictEqual(read(where.exe), 'old agent-record');
  assert.strictEqual(read(path.join(where.root, 'secrets.json')), '[{"name":"x"}]');
  assert.strictEqual(read(path.join(where.root, 'recorder.json')), '{"video":true}');
  assert.ok(fs.existsSync(path.join(where.state, 'recordings', 'r.json')));
  assert.notStrictEqual(read(path.join(oldSkill, 'SKILL.md')), 'old', 'the earlier skill is replaced');
  uninstall(['claude-code'], { ...options, removeProgram: true });
  assert.strictEqual(read(path.join(where.root, 'secrets.json')), '[{"name":"x"}]', 'uninstall keeps saved passwords');
});

test('OpenCode is set up in its own format, and Windsurf is no longer offered', () => {
  const { agents } = require('../lib/agents');
  const { home, env, options } = fakeHome();
  const dir = path.join(home, '.config', 'opencode');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'opencode.json'), JSON.stringify({ theme: 'x', mcp: { servers: { other: { type: 'remote', url: 'u' } } } }));
  const ids = agents(home, env).map((a) => a.id);
  assert.ok(ids.includes('opencode') && !ids.includes('windsurf'), ids.join(','));
  assert.ok(agents(home, env).find((a) => a.id === 'opencode').detect());
  const [line] = install(['opencode'], options);
  assert.doesNotMatch(line, /not set up/, line);
  const where = layout(home);
  const data = JSON.parse(read(path.join(dir, 'opencode.json')));
  assert.strictEqual(data.theme, 'x');
  assert.ok(data.mcp.servers.other);
  assert.deepStrictEqual(data.mcp.servers[SERVER], {
    type: 'local', command: [where.exe, 'mcp'],
    environment: { AGENT_DESKTOP_HOME: where.state },
  });
  assert.ok(fs.existsSync(path.join(dir, 'skills', 'desktop-recording', 'SKILL.md')), 'OpenCode gets the skills');
  assert.ok(status(options).some((l) => /^OpenCode: found, MCP set up, skills installed/.test(l)));
  uninstall(['opencode'], options);
  assert.ok(!JSON.parse(read(path.join(dir, 'opencode.json'))).mcp.servers[SERVER]);
  assert.ok(!fs.existsSync(path.join(dir, 'skills', 'desktop-recording')));
});
