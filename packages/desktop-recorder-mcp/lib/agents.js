// Part of Agent Record Adapter (see NOTICE).
'use strict';

// The agents the installer knows: how to tell one is installed, where its MCP servers are configured,
// and where it reads skills from. An agent with no skills folder still gets the guides through the MCP
// server itself (agent-record serves them as its skills tools).

const path = require('path');
const fs = require('fs');

function onPath(command, env) {
  const dirs = (env.PATH || env.Path || '').split(path.delimiter).filter(Boolean);
  const exts = (env.PATHEXT || '.EXE;.CMD;.BAT;.PS1').split(';').map((e) => e.toLowerCase()).concat(['']);
  return dirs.some((dir) => exts.some((ext) => fs.existsSync(path.join(dir, command + ext))));
}

/**
 * Where Claude Desktop reads its MCP servers. Recent versions keep their data in
 * %LOCALAPPDATA%\Claude-Data\<package folder> and leave a ".user-data-left" marker where it used to be;
 * older ones use the packaged app's LocalCache\Roaming\Claude, or %APPDATA%\Claude.
 */
function claudeDesktopConfig(home, env) {
  const local = env.LOCALAPPDATA || path.join(home, 'AppData', 'Local');
  const data = path.join(local, 'Claude-Data');
  try {
    const found = fs.readdirSync(data, { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && /^claude_/i.test(entry.name))
      .map((entry) => path.join(data, entry.name))
      .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);
    if (found.length) return path.join(found[0], 'claude_desktop_config.json');
  } catch (_) {
    /* an older version */
  }
  const packages = path.join(local, 'Packages');
  let packaged = null;
  try {
    const found = fs.readdirSync(packages).find((name) => /^Claude_/i.test(name));
    if (found) packaged = path.join(packages, found, 'LocalCache', 'Roaming', 'Claude');
  } catch (_) {
    /* no packaged install */
  }
  const roaming = path.join(env.APPDATA || path.join(home, 'AppData', 'Roaming'), 'Claude');
  const dir = packaged && fs.existsSync(packaged) ? packaged : roaming;
  return path.join(dir, 'claude_desktop_config.json');
}

/** Every agent, described for one home folder and environment. */
function agents(home, env) {
  const appData = env.APPDATA || path.join(home, 'AppData', 'Roaming');
  return [
    {
      id: 'claude-code',
      name: 'Claude Code',
      cli: 'claude',
      detect: () => fs.existsSync(path.join(home, '.claude')) || onPath('claude', env),
      mcp: { kind: 'json', file: path.join(home, '.claude.json'), key: 'mcpServers', type: 'stdio' },
      skillsDir: path.join(home, '.claude', 'skills'),
    },
    {
      id: 'codex',
      name: 'Codex (the codex command and the Codex app)',
      cli: 'codex',
      detect: () => fs.existsSync(path.join(home, '.codex')) || onPath('codex', env),
      mcp: { kind: 'toml', file: path.join(home, '.codex', 'config.toml') },
      skillsDir: path.join(home, '.codex', 'skills'),
    },
    {
      id: 'gemini',
      name: 'Gemini CLI',
      detect: () => fs.existsSync(path.join(home, '.gemini')) || onPath('gemini', env),
      mcp: { kind: 'json', file: path.join(home, '.gemini', 'settings.json'), key: 'mcpServers' },
      skillsDir: null,
    },
    {
      id: 'claude-desktop',
      name: 'Claude Desktop',
      detect: () => fs.existsSync(path.dirname(claudeDesktopConfig(home, env))),
      mcp: { kind: 'json', file: claudeDesktopConfig(home, env), key: 'mcpServers' },
      skillsDir: null,
    },
    {
      id: 'cursor',
      name: 'Cursor',
      detect: () => fs.existsSync(path.join(home, '.cursor')) || onPath('cursor', env),
      mcp: { kind: 'json', file: path.join(home, '.cursor', 'mcp.json'), key: 'mcpServers' },
      skillsDir: null,
    },
    {
      id: 'vscode',
      name: 'VS Code (Copilot)',
      detect: () => fs.existsSync(path.join(appData, 'Code', 'User')) || onPath('code', env),
      mcp: { kind: 'json', file: path.join(appData, 'Code', 'User', 'mcp.json'), key: 'servers', type: 'stdio' },
      skillsDir: null,
    },
    {
      id: 'opencode',
      name: 'OpenCode',
      detect: () => fs.existsSync(openCodeDir(home)) || onPath('opencode', env),
      mcp: { kind: 'opencode', file: openCodeConfig(home) },
      skillsDir: path.join(openCodeDir(home), 'skills'),
    },
  ];
}

/** OpenCode's global folder: ~/.config/opencode on every system, Windows included. */
function openCodeDir(home) {
  return path.join(home, '.config', 'opencode');
}

/** OpenCode reads opencode.json or opencode.jsonc there: the one that exists, else opencode.json. */
function openCodeConfig(home) {
  const jsonc = path.join(openCodeDir(home), 'opencode.jsonc');
  const json = path.join(openCodeDir(home), 'opencode.json');
  return fs.existsSync(jsonc) && !fs.existsSync(json) ? jsonc : json;
}

module.exports = { agents, onPath };
