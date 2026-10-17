// Hosted Windows only. Attaches to the actual Tauri WebView2 control through CDP.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { chromium } = require('playwright-core');

const outputDir = path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'demostrike-smoke');
const fixturePath = path.join(outputDir, 'synthetic-library.sqlite');
const appPath = path.resolve(__dirname, '../src-tauri/target/release/demostrike-rust-candidate.exe');
const cdpPort = 19373;
const cdpUrl = `http://127.0.0.1:${cdpPort}`;

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function waitForCdp(app, getExit) {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    if (getExit() !== null) throw new Error(`Tauri exited before WebView2 opened CDP: ${getExit()}`);
    try {
      const response = await fetch(`${cdpUrl}/json/version`, { signal: AbortSignal.timeout(1500) });
      if (response.ok) return;
    } catch { /* WebView2 is still starting. */ }
    await sleep(1000);
  }
  throw new Error('Tauri WebView2 CDP endpoint did not open within 60 seconds');
}

async function findAppPage(browser) {
  for (let attempt = 0; attempt < 30; attempt += 1) {
    for (const context of browser.contexts()) {
      for (const page of context.pages()) {
        try {
          if (await page.evaluate(() => Boolean(window.__TAURI_INTERNALS__))) return page;
        } catch { /* A target may still be navigating. */ }
      }
    }
    await sleep(1000);
  }
  const urls = browser.contexts().flatMap((context) => context.pages().map((page) => page.url()));
  throw new Error(`No Tauri IPC-enabled WebView2 page appeared. CDP targets: ${urls.join(', ')}`);
}

async function capture(page, name) {
  const file = path.join(outputDir, `synthetic-${name}.png`);
  await page.screenshot({ path: file, animations: 'disabled' });
  assert.ok(fs.statSync(file).size > 10000, `App content screenshot is unexpectedly small: ${file}`);
  return { file: path.basename(file), sha256: sha256(file), bytes: fs.statSync(file).size };
}

async function main() {
  assert.ok(process.platform === 'win32', 'Native smoke runs only on hosted Windows');
  assert.ok(fs.existsSync(appPath), `Compiled Tauri executable missing: ${appPath}`);
  assert.ok(fs.existsSync(fixturePath), `Synthetic fixture missing: ${fixturePath}`);
  const fixtureHash = sha256(fixturePath);
  const app = spawn(appPath, [], {
    cwd: path.dirname(appPath),
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${cdpPort}`,
      WEBVIEW2_USER_DATA_FOLDER: path.join(outputDir, 'webview2-profile'),
    },
    windowsHide: false,
    stdio: 'pipe',
  });
  let exit = null;
  app.once('exit', (code, signal) => { exit = `${code ?? 'no code'} / ${signal ?? 'no signal'}`; });
  app.stdout.on('data', (data) => process.stdout.write(data));
  app.stderr.on('data', (data) => process.stderr.write(data));
  let browser;
  try {
    await waitForCdp(app, () => exit);
    browser = await chromium.connectOverCDP(cdpUrl, { timeout: 15000 });
    const page = await findAppPage(browser);
    const pageErrors = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await page.getByRole('heading', { name: 'Matches' }).waitFor({ timeout: 20000 });
    const runtimeUrl = page.url();

    await page.getByRole('textbox', { name: 'Library file' }).fill(fixturePath);
    await page.getByRole('button', { name: 'Open library' }).click();
    await page.getByRole('button', { name: 'synthetic-alpha.dem' }).waitFor({ timeout: 20000 });
    await page.locator('.detail-header h2').filter({ hasText: 'de_mirage' }).waitFor({ timeout: 20000 });
    assert.equal(await page.getByRole('button', { name: 'synthetic-beta.dem' }).count(), 1);
    assert.equal(await page.locator('.matches-table tbody tr').count(), 2);
    assert.equal(await page.getByText('synthetic-ci-only').count(), 1);
    const screenshots = [await capture(page, 'overview')];

    await page.getByRole('tab', { name: 'Rounds' }).click();
    assert.equal(await page.locator('.tab-content tbody tr').count(), 3);
    assert.ok((await page.getByRole('tabpanel').innerText()).includes('Unknown'));
    screenshots.push(await capture(page, 'rounds'));

    await page.getByRole('tab', { name: 'Players' }).click();
    assert.equal(await page.locator('.tab-content tbody tr').count(), 2);
    assert.ok((await page.getByRole('tabpanel').innerText()).includes('Synthetic CT'));

    await page.getByRole('tab', { name: 'Utility' }).click();
    assert.equal(await page.locator('.tab-content tbody tr').count(), 3);
    await page.getByRole('combobox', { name: 'Round' }).selectOption('2');
    assert.equal(await page.locator('.tab-content tbody tr').count(), 1);
    assert.ok((await page.getByRole('tabpanel').innerText()).includes('flash'));
    screenshots.push(await capture(page, 'utility-round-2'));

    await page.getByRole('button', { name: 'synthetic-beta.dem' }).click();
    await page.locator('.detail-header h2').filter({ hasText: 'de_inferno' }).waitFor({ timeout: 20000 });
    assert.equal(await page.getByRole('tab', { name: 'Overview' }).getAttribute('aria-selected'), 'true');
    assert.equal(sha256(fixturePath), fixtureHash, 'Tauri changed the read-only fixture bytes');
    assert.deepEqual(pageErrors, [], `WebView2 page errors: ${pageErrors.join('; ')}`);

    const proof = {
      source: 'actual hosted Tauri WebView2 control, not a browser mock',
      fixture: 'sanitized deterministic synthetic SQLite; no real demo data',
      fixtureSha256: fixtureHash,
      commit: process.env.GITHUB_SHA || null,
      executableSha256: sha256(appPath),
      processId: app.pid,
      runtimeUrl,
      tauriIpcAvailable: true,
      assertions: ['two matches loaded through IPC', 'overview', 'three rounds', 'two players', 'three utility events', 'round filter', 'second match selection', 'database bytes unchanged'],
      screenshots,
    };
    fs.writeFileSync(path.join(outputDir, 'synthetic-smoke-proof.json'), `${JSON.stringify(proof, null, 2)}\n`);
    console.log(`PASS: native Tauri WebView2 IPC smoke; ${screenshots.map((item) => item.file).join(', ')}`);
  } finally {
    if (browser) await browser.close().catch(() => {});
    if (exit === null) app.kill();
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
