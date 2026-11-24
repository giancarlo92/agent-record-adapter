// Part of Agent Record Adapter (see NOTICE).
'use strict';

// Publishes this version and waits until anyone can install it.
//
//   npm run release
//
// npm accepts a publish at once but, for accounts with staged publishing, shows the new version only
// some minutes later, and its download a few minutes after that. Until then `npx agent-record-adapter@<v>`
// fails with ETARGET (no such version) or a 404. This waits for both, so the install command it
// prints at the end works. A version already published and still waiting (E409) is waited for too.

const { spawn } = require('child_process');
const https = require('https');
const path = require('path');

const pkg = require(path.join(__dirname, '..', 'package.json'));
const name = pkg.name;
const version = pkg.version;
const STEP_MS = 20_000;
const LIMIT_MS = 30 * 60_000;

function status(url) {
  return new Promise((resolve) => {
    const request = https.get(url, { headers: { accept: 'application/json', 'cache-control': 'no-cache' } }, (response) => {
      let body = '';
      response.on('data', (chunk) => { body += chunk; });
      response.on('end', () => resolve({ code: response.statusCode, body }));
    });
    request.on('error', () => resolve({ code: 0, body: '' }));
    request.setTimeout(15_000, () => request.destroy());
  });
}

async function published() {
  const meta = await status(`https://registry.npmjs.org/${name}?t=${Date.now()}`);
  let listed = false;
  try {
    listed = Boolean(JSON.parse(meta.body).versions[version]);
  } catch (_) { /* not yet */ }
  const tarball = await status(`https://registry.npmjs.org/${name}/-/${name}-${version}.tgz`);
  return { listed, downloadable: tarball.code === 200 };
}

/** `npm publish`, with its output shown as it comes (it may ask to confirm in the browser) and kept. */
function publish() {
  // npm on Windows is a .cmd script: run it through cmd.exe as one command line (no shell: true,
  // which Node 24 warns about with an argument list).
  const [command, args] = process.platform === 'win32'
    ? [process.env.ComSpec || 'cmd.exe', ['/d', '/s', '/c', 'npm publish']]
    : ['npm', ['publish']];
  return new Promise((resolve) => {
    const child = spawn(command, args, { cwd: path.join(__dirname, '..'), stdio: ['inherit', 'inherit', 'pipe'] });
    let stderr = '';
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
      process.stderr.write(chunk);
    });
    child.on('close', (code) => resolve({ code, stderr }));
  });
}

async function main() {
  const before = await published();
  if (before.listed && before.downloadable) {
    console.log(`${name}@${version} is already published and can be installed.`);
  } else {
    console.log(`Publishing ${name}@${version} (npm may open the browser to confirm it is you)...`);
    const result = await publish();
    const staged = /E409|previously staged/.test(result.stderr);
    if (result.code !== 0 && !staged) {
      console.error('\nNot published. Nothing to wait for. (If npm said 401 or 404: run npm login, then npm run release again.)');
      process.exit(1);
    }
    if (staged) console.log(`\n${version} was already published and is still waiting on npm's side: waiting for it.`);
  }

  const started = Date.now();
  console.log(`\nWaiting until ${name}@${version} can be installed (usually 3 to 8 minutes). Do not run npx before.`);
  for (;;) {
    const now = await published();
    const minutes = ((Date.now() - started) / 60_000).toFixed(1);
    if (now.listed && now.downloadable) {
      console.log(`\nReady after ${minutes} min. Install it from any folder but this one:\n\n  cd ~\n  npx ${name}@${version}\n`);
      return;
    }
    console.log(`  ${minutes} min: ${now.listed ? 'version listed' : 'version not listed yet'}, ${now.downloadable ? 'download ready' : 'download not ready yet'}`);
    if (Date.now() - started > LIMIT_MS) {
      console.error(`\nStill not ready after ${LIMIT_MS / 60_000} minutes. Check https://www.npmjs.com/package/${name}?activeTab=versions`);
      process.exit(1);
    }
    await new Promise((done) => setTimeout(done, STEP_MS));
  }
}

main();
