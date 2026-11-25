// Part of Agent Record Adapter (see NOTICE).
// Measures check-scripts: for each case, what the check says (without running), then what really
// happens when the script runs (on the hidden work desktop), and whether they agree.
// The scripts are the recorded ones of the Aprende Replica tests (%USERPROFILE%\AprendeReplica-e2e):
// test pages and WordCraft only. Needs a debug build (cargo build -p agent-record) and the
// installed desktop-recorder-mcp programs. Cases and results go to %TEMP%\check-scripts-battery.
//   node check-scripts-battery.mjs [--only id,id] [--check-only]
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const SCRIPT_DIR = dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));
const HERE = join(tmpdir(), 'check-scripts-battery');
const HOME = homedir();
const REPO = resolve(SCRIPT_DIR, '..', '..', '..');
const CHECKER = join(REPO, 'target', 'debug', 'agent-record.exe');
const ENGINE = join(HOME, '.desktop-recorder-mcp', 'bin', 'agent-desktop.exe');
const CURRENT_HELPER = join(REPO, 'skills', 'recording-to-script', 'scripts', 'desktop-script.mjs');
const FORK_HELPER = join(HOME, 'Downloads', 'Recording 2026-11-19 12.18.01', 'desktop-script.mjs');
const E2E = join(HOME, 'AprendeReplica-e2e');
const FORMS = process.env.FORMS_URL || pathToFileURL(join(HOME, 'OneDrive', 'Documentos', 'Proyectos', 'Personales', 'Proyecto Aprende Replica', 'app', 'testdata', 'forms')).href;
const SIGNUP_ARGS = ['--full-name', 'Lucia Fernandez', '--email', 'lucia@northwind.example', '--phone', '+34 600 123 456',
  '--birth-date', '1990-05-14', '--country', 'Peru', '--region', 'Lima', '--username', 'lucia_f',
  '--about', 'We automate the monthly invoices of our shop.'];

const bases = {
  sgh: { dir: join(E2E, 'enter-test', 'script-sgh'), script: 'script.mjs', args: [],
    ui: ['name: "Personal2"', 'name: "Personal9"'] },
  studio: { dir: join(E2E, 'app-steps-home', 'script-check'), script: 'script.mjs', args: [] },
  signupRec: { dir: join(E2E, 'recorder-app', 'home', 'Documents', 'Desktop Recorder', 'Recording 2026-11-18 17.41.00'),
    script: 'script.mjs', args: [], ui: ['nativeId: "fullName"', 'nativeId: "fullNameRenamed"'] },
  signup13: { dir: join(E2E, 'data', 'js-script'), script: 'sign-up.mjs',
    args: ['--url', `${FORMS}/signup-v1.html`, ...SIGNUP_ARGS], ui: ["nativeId: 'fullName'", "nativeId: 'fullNameRenamed'"] },
  signupRepaired: { dir: join(E2E, 'data', 'js-script'), script: 'sign-up-repaired.mjs',
    args: ['--url', `${FORMS}/signup-v2.html`, ...SIGNUP_ARGS, '--document-id', 'ABC-123456'] },
  npx012: { dir: join(E2E, 'npx-012'), script: 'sign-up.mjs', args: ['--url', `${FORMS}/signup-v1.html`, ...SIGNUP_ARGS] },
  wordcraft: { dir: join(E2E, 'enter-test', 'app-recs', 'Recording 2026-11-24 03.44.22'), script: 'script.mjs', args: [] },
};

// Breaks an update of ours could bring, made in the helper copy (or the script for `ui`).
const mutations = {
  unknownCommand: { what: 'a command renamed upstream (set-value -> set-text)', helper: ["run('set-value', this.ref, String(value))", "run('set-text', this.ref, String(value))"] },
  unknownOption: { what: 'an option that no longer exists (click --force)', helper: ["return run('click', this.ref);", "return run('click', this.ref, '--force');"] },
  renamedOption: { what: 'an option renamed (find --window-id -> --window)', helper: ["const args = ['find', '--window-id', this.id,", "const args = ['find', '--window', this.id,"] },
  invalidValue: { what: 'a value the parser refuses (find --limit many)', helper: ["const args = ['find', '--window-id', this.id,", "const args = ['find', '--limit', 'many', '--window-id', this.id,"] },
  missingRequired: { what: 'a required argument dropped (click without its ref)', helper: ["return run('click', this.ref);", "return run('click');"] },
  optionalDropped: { what: 'set-value without its value (the parser allows it: the value may come from stdin)', helper: ["run('set-value', this.ref, String(value))", "run('set-value', this.ref)"] },
  missingProgram: { what: 'the program moved (helper looks in an old folder)', helper: ["join(homedir(), '.desktop-recorder-mcp', 'bin', exe)", "join(homedir(), '.old-install-folder', 'bin', exe)"] },
  oldHelper: { what: 'the helper copy from the old fork', replaceHelper: FORK_HELPER },
  ui: { what: 'the page changed (an element renamed): not about our system', ui: true },
};

const cases = [];
for (const [id, base] of Object.entries(bases)) {
  cases.push({ id: `found:${id}`, group: 'as found', base, helper: 'own' });
  cases.push({ id: `current:${id}`, group: 'current helper', base, helper: 'current' });
}
for (const id of ['sgh', 'signupRec', 'signup13']) {
  for (const [mid, m] of Object.entries(mutations)) {
    if (m.ui && !bases[id].ui) continue;
    cases.push({ id: `${mid}:${id}`, group: 'mutation', base: bases[id], helper: 'current', mutation: m, mid });
  }
}

const argv = process.argv.slice(2);
const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1].split(',') : null;
const checkOnly = argv.includes('--check-only');
const selected = cases.filter((c) => !only || only.some((o) => c.id === o || c.id.startsWith(o + ':') || c.id.endsWith(':' + o)));

function replaceOnce(text, [from, to], where) {
  if (!text.includes(from)) throw new Error(`${where}: "${from}" not found`);
  return text.replace(from, to);
}

function build(c) {
  const dir = join(HERE, 'cases', c.id.replace(/:/g, '-'));
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  copyFileSync(join(c.base.dir, c.base.script), join(dir, c.base.script));
  const helperFrom = c.mutation?.replaceHelper || (c.helper === 'current' ? CURRENT_HELPER : join(c.base.dir, 'desktop-script.mjs'));
  let helper = readFileSync(helperFrom, 'utf8');
  if (c.mutation?.helper) helper = replaceOnce(helper, c.mutation.helper, c.id);
  writeFileSync(join(dir, 'desktop-script.mjs'), helper);
  if (c.mutation?.ui) {
    const file = join(dir, c.base.script);
    writeFileSync(file, replaceOnce(readFileSync(file, 'utf8'), c.base.ui, c.id));
  }
  return dir;
}

function check(dir, script) {
  let out;
  try {
    out = execFileSync(CHECKER, ['check-scripts', dir], { encoding: 'utf8', env: { ...process.env, AGENT_RECORD_ENGINE: ENGINE }, maxBuffer: 64 << 20 });
  } catch (error) {
    out = error.stdout;
  }
  const result = JSON.parse(out);
  const entry = result.data.scripts.find((s) => basename(s.path) === script);
  const helperEntry = result.data.scripts.find((s) => basename(s.path) === 'desktop-script.mjs');
  return {
    verdict: entry?.verdict ?? 'not found',
    errors: (entry?.findings || []).filter((f) => f.severity === 'error').map((f) => f.message),
    helperErrors: (helperEntry?.findings || []).filter((f) => f.severity === 'error').map((f) => `${f.line}: ${f.message}`),
  };
}

function processes(filter) {
  try {
    const out = execFileSync('powershell.exe', ['-NoProfile', '-Command',
      `Get-CimInstance Win32_Process | Where-Object { ${filter} } | ForEach-Object { $_.ProcessId }`], { encoding: 'utf8' });
    return out.split(/\s+/).filter(Boolean).map(Number);
  } catch (_) {
    return [];
  }
}

const OURS = "($_.Name -eq 'wordcraft.exe') -or ($_.CommandLine -like '*script-browser*')";

function closeStarted(before) {
  for (const pid of processes(OURS)) {
    if (before.includes(pid)) continue;
    try { execFileSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' }); } catch (_) { /* gone */ }
  }
}

const COMPAT = /INVALID_ARGS|unrecognized subcommand|unexpected argument|invalid value|is not agent-record|is not the background-only|was not found: run "npx|required arguments were not provided|a value is required|ENOENT/;

function runScript(dir, c) {
  return new Promise((resolve) => {
    const before = processes(OURS);
    const child = spawn(process.execPath, [join(dir, c.base.script), ...c.base.args, '--yes'], {
      cwd: dir,
      env: { ...process.env, AGENT_DESKTOP_WORK_DESKTOP: 'artest' },
      windowsHide: true,
    });
    let text = '';
    child.stdout.on('data', (d) => { text += d; });
    child.stderr.on('data', (d) => { text += d; });
    const timer = setTimeout(() => { text += '\n[timeout]'; child.kill(); }, 240000);
    child.on('close', (code) => {
      clearTimeout(timer);
      closeStarted(before);
      const failure = text.split('\n').find((line) => /failed|Error|error/.test(line)) || '';
      const outcome = code === 0 ? 'works' : COMPAT.test(text) ? 'fails: our system' : 'fails: other';
      resolve({ code, outcome, failure: failure.trim().slice(0, 300), tail: text.trim().split('\n').slice(-4).join(' | ').slice(0, 500) });
    });
  });
}

const resultsFile = join(HERE, 'results.json');
const results = existsSync(resultsFile) ? JSON.parse(readFileSync(resultsFile, 'utf8')) : {};
for (const c of selected) {
  const dir = build(c);
  const checked = check(dir, c.base.script);
  const entry = { id: c.id, group: c.group, what: c.mutation?.what || (c.helper === 'current' ? 'current helper' : 'as found'), check: checked };
  if (!checkOnly) entry.run = await runScript(dir, c);
  results[c.id] = entry;
  writeFileSync(resultsFile, JSON.stringify(results, null, 2));
  console.log(`${c.id.padEnd(30)} check: ${checked.verdict.padEnd(14)} run: ${entry.run ? entry.run.outcome : '-'}${entry.run && entry.run.outcome !== 'works' ? '  ' + entry.run.failure.slice(0, 160) : ''}`);
}
