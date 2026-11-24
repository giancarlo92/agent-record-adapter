// Part of Agent Record Adapter (see NOTICE).
'use strict';

// Adding and removing one MCP server in an agent's configuration file, keeping everything else in it.
// A file that cannot be read safely (comments in JSON, a hand-written TOML block we do not recognise)
// is left untouched and reported, never overwritten. Every change keeps a backup next to the file.

const fs = require('fs');
const path = require('path');

const BACKUP_SUFFIX = '.bak-desktop-recorder-mcp';

function backup(file) {
  if (fs.existsSync(file) && !fs.existsSync(file + BACKUP_SUFFIX)) {
    fs.copyFileSync(file, file + BACKUP_SUFFIX);
  }
}

function readJson(file) {
  if (!fs.existsSync(file)) return {};
  const text = fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, '');
  if (!text.trim()) return {};
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(`${file} is not plain JSON (it may have comments); add the server by hand: ${error.message}`);
  }
}

function writeJson(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  backup(file);
  fs.writeFileSync(file, JSON.stringify(data, null, 2) + '\n');
}

function jsonEntry(server, type) {
  const entry = {};
  if (type) entry.type = type;
  entry.command = server.command;
  entry.args = server.args;
  if (server.env && Object.keys(server.env).length) entry.env = server.env;
  return entry;
}

function addJson(mcp, name, server) {
  const data = readJson(mcp.file);
  data[mcp.key] = data[mcp.key] || {};
  data[mcp.key][name] = jsonEntry(server, mcp.type);
  writeJson(mcp.file, data);
}

function removeJson(mcp, name) {
  if (!fs.existsSync(mcp.file)) return false;
  const data = readJson(mcp.file);
  if (!data[mcp.key] || !(name in data[mcp.key])) return false;
  delete data[mcp.key][name];
  writeJson(mcp.file, data);
  return true;
}

function hasJson(mcp, name) {
  try {
    const data = readJson(mcp.file);
    return Boolean(data[mcp.key] && data[mcp.key][name]);
  } catch (_) {
    return false;
  }
}

// TOML: the server is one block of its own, [mcp_servers.<name>] and its env table, written and
// removed as a whole.
function tomlString(value) {
  return "'" + String(value).replace(/'/g, "''") + "'";
}

function tomlBlock(name, server) {
  const lines = [`[mcp_servers.${name}]`, `command = ${tomlString(server.command)}`,
    `args = [${server.args.map(tomlString).join(', ')}]`];
  if (server.env && Object.keys(server.env).length) {
    lines.push('', `[mcp_servers.${name}.env]`);
    for (const [key, value] of Object.entries(server.env)) lines.push(`${key} = ${tomlString(value)}`);
  }
  return lines.join('\n') + '\n';
}

function stripToml(text, name) {
  const header = new RegExp(`^\\[mcp_servers\\.${name.replace(/[-.]/g, '\\$&')}(\\.[^\\]]+)?\\]\\s*$`);
  const out = [];
  let inside = false;
  for (const line of text.split(/\r?\n/)) {
    if (/^\s*\[/.test(line)) inside = header.test(line.trim());
    if (!inside) out.push(line);
  }
  return out.join('\n').replace(/\n{3,}/g, '\n\n');
}

function addToml(mcp, name, server) {
  const text = fs.existsSync(mcp.file) ? fs.readFileSync(mcp.file, 'utf8') : '';
  const kept = stripToml(text, name).replace(/\s*$/, '');
  fs.mkdirSync(path.dirname(mcp.file), { recursive: true });
  backup(mcp.file);
  fs.writeFileSync(mcp.file, (kept ? kept + '\n\n' : '') + tomlBlock(name, server));
}

function removeToml(mcp, name) {
  if (!fs.existsSync(mcp.file)) return false;
  const text = fs.readFileSync(mcp.file, 'utf8');
  const kept = stripToml(text, name);
  if (kept === text) return false;
  backup(mcp.file);
  fs.writeFileSync(mcp.file, kept.replace(/\s*$/, '') + '\n');
  return true;
}

function hasToml(mcp, name) {
  if (!fs.existsSync(mcp.file)) return false;
  return fs.readFileSync(mcp.file, 'utf8').includes(`[mcp_servers.${name}]`);
}

// OpenCode: its servers are under "mcp" > "servers", each a local command given as one list (the
// program, then its arguments) with its variables under "environment".
function addOpenCode(mcp, name, server) {
  const data = readJson(mcp.file);
  if (!data.$schema) data.$schema = 'https://opencode.ai/config.json';
  data.mcp = data.mcp || {};
  data.mcp.servers = data.mcp.servers || {};
  const entry = { type: 'local', command: [server.command, ...server.args] };
  if (server.env && Object.keys(server.env).length) entry.environment = server.env;
  data.mcp.servers[name] = entry;
  writeJson(mcp.file, data);
}

function removeOpenCode(mcp, name) {
  if (!fs.existsSync(mcp.file)) return false;
  const data = readJson(mcp.file);
  const servers = data.mcp && data.mcp.servers;
  if (!servers || !(name in servers)) return false;
  delete servers[name];
  writeJson(mcp.file, data);
  return true;
}

function hasOpenCode(mcp, name) {
  try {
    const data = readJson(mcp.file);
    return Boolean(data.mcp && data.mcp.servers && data.mcp.servers[name]);
  } catch (_) {
    return false;
  }
}

const KINDS = {
  json: { add: addJson, remove: removeJson, has: hasJson },
  toml: { add: addToml, remove: removeToml, has: hasToml },
  opencode: { add: addOpenCode, remove: removeOpenCode, has: hasOpenCode },
};
const add = (mcp, name, server) => KINDS[mcp.kind].add(mcp, name, server);
const remove = (mcp, name) => KINDS[mcp.kind].remove(mcp, name);
const has = (mcp, name) => KINDS[mcp.kind].has(mcp, name);

module.exports = { add, remove, has, tomlBlock, stripToml, BACKUP_SUFFIX };
