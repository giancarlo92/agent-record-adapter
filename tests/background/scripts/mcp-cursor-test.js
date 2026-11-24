// Part of Agent Record Adapter (see NOTICE). Ported from the desktop-recorder-mcp fork.
// The MCP cursor switch, live: an MCP client drives `agent-desktop mcp` on the SGH-like test page.
// node mcp-cursor-test.js <agent-record.exe> <window-id> <where.ps1>
const { spawn, execFileSync } = require('child_process');
const [exe, win, wherePs] = process.argv.slice(2);
const server = spawn(exe, ['mcp'], { stdio: ['pipe', 'pipe', 'inherit'], env: process.env });
let buf = '', id = 0; const waiting = new Map();
server.stdout.on('data', d => { buf += d; let i; while ((i = buf.indexOf('\n')) >= 0) { const line = buf.slice(0, i); buf = buf.slice(i + 1); try { const m = JSON.parse(line); if (waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); } } catch {} } });
const rpc = (method, params) => new Promise(r => { const n = ++id; waiting.set(n, r); server.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: n, method, params }) + '\n'); });
const tool = async (name, args = {}) => { const m = await rpc('tools/call', { name, arguments: args }); return m.result.structuredContent || { text: m.result.content[0].text, isError: m.result.isError }; };
const where = () => execFileSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', wherePs]).toString().trim();
const refOf = (snap, test) => { let h = ''; (function w(n) { if (!n || h) return; if (test(n) && n.ref_id) h = n.ref_id; (n.children || []).forEach(w); })(snap.data && snap.data.tree); return h; };
(async () => {
  await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'test', version: '1' } });
  const step = (label, v) => console.log(label.padEnd(34), typeof v === 'string' ? v : JSON.stringify(v).slice(0, 260));
  step('start (nothing asked):', where());
  let snap, before = '';
  for (let i = 0; i < 15 && !before; i++) { snap = await tool('desktop_snapshot', { window_id: win, interactive_only: true }); before = refOf(snap, n => n.role === 'radiobutton' && n.name === 'Pago directo'); if (!before) await new Promise(r => setTimeout(r, 1000)); }
  step('first snapshot ref:', before);
  let r = await tool('desktop_click', { ref_id: before });
  step('click, cursor off by default:', (r.ok ? 'ok' : 'ERR ' + r.error.code) + ' | ' + where());
  r = await tool('desktop_cursor_overlay_disable');
  step('disable while off:', r.data);
  r = await tool('desktop_cursor_overlay_enable', { label: 'Filling the form' });
  step('enable:', r.ok ? r.data.mcp : r);
  r = await tool('desktop_click', { ref_id: before });
  step('old ref after enable:', r.ok ? 'ok' : r.error.code);
  snap = await tool('desktop_snapshot', { window_id: win, interactive_only: true });
  const radio = refOf(snap, n => n.role === 'radiobutton' && n.name === 'Pago directo');
  r = await tool('desktop_click', { ref_id: radio });
  step('click "Pago directo", cursor on:', (r.ok ? 'ok' : 'ERR ' + r.error.code) + ' | ' + where());
  const opt = refOf(snap, n => n.role === 'option' && /^Personal/.test(n.name || ''));
  r = await tool('desktop_click', { ref_id: opt });
  step('click "Personal", cursor on:', (r.ok ? 'ok' : 'ERR ' + r.error.code) + ' | ' + where());
  r = await tool('desktop_cursor_overlay_disable');
  step('disable:', r.ok ? r.data.mcp : r);
  await new Promise(res => setTimeout(res, 800));
  step('after disable:', where());
  r = await tool('desktop_click', { ref_id: radio });
  step('same ref after disable:', (r.ok ? 'ok' : 'ERR ' + r.error.code) + ' | ' + where());
  r = await tool('desktop_cursor_overlay_enable', {});
  step('enable again:', r.ok ? r.data.mcp : r);
  r = await tool('desktop_click', { ref_id: radio });
  step('click again:', (r.ok ? 'ok' : 'ERR ' + r.error.code) + ' | ' + where());
  server.stdin.end();
  await new Promise(res => server.on('exit', res));
  await new Promise(res => setTimeout(res, 1500));
  step('server closed:', where());
})();
