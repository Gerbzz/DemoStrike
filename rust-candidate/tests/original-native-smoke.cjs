// Hosted Windows only: capture the preserved CS Demo Manager React renderer
// inside a real Tauri WebView2 window after its Rust startup path completes.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { chromium } = require('playwright-core');

const outputDir = path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'demostrike-original');
const appPath = path.resolve(__dirname, '../src-tauri/target/release/demostrike-rust-candidate.exe');
const cdpUrl = 'http://127.0.0.1:19375';
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');

async function main() {
  assert.equal(process.platform, 'win32');
  assert.ok(fs.existsSync(appPath), `Native app missing: ${appPath}`);
  fs.mkdirSync(outputDir, { recursive: true });
  const app = spawn(appPath, [], { cwd: path.dirname(appPath), windowsHide: false, stdio: 'pipe' });
  let exit = null;
  app.once('exit', (code, signal) => { exit = `${code ?? 'none'} / ${signal ?? 'none'}`; });
  app.stdout.on('data', (data) => process.stdout.write(data));
  app.stderr.on('data', (data) => process.stderr.write(data));
  let browser;
  let page;
  try {
    let ready = false;
    for (let attempt = 0; attempt < 60; attempt += 1) {
      if (exit !== null) throw new Error(`App exited before WebView2 opened: ${exit}`);
      try {
        const response = await fetch(`${cdpUrl}/json/version`, { signal: AbortSignal.timeout(1500) });
        if (response.ok) { ready = true; break; }
      } catch { /* Native startup still in progress. */ }
      await sleep(1000);
    }
    assert.ok(ready, 'Native WebView2 debugging endpoint did not open');
    browser = await chromium.connectOverCDP(cdpUrl, { timeout: 15000 });
    for (let attempt = 0; attempt < 30 && !page; attempt += 1) {
      for (const context of browser.contexts()) {
        for (const target of context.pages()) {
          try {
            if (await target.evaluate(() => Boolean(window.__TAURI_INTERNALS__))) page = target;
          } catch { /* Page is navigating. */ }
        }
      }
      if (!page) await sleep(1000);
    }
    assert.ok(page, 'No Tauri IPC-enabled page appeared');
    const pageErrors = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await page.getByText('Your local library is empty').waitFor({ timeout: 25000 });
    await page.getByText('Demo import and populated match data are still being ported.').waitFor({ timeout: 25000 });
    assert.equal(await page.locator('a[href="#/matches"]').count(), 1, 'Original Matches sidebar route missing');
    assert.equal(await page.locator('a[href="#/demos"]').count(), 1, 'Original Demos sidebar route missing');
    assert.equal(await page.locator('a[href="#/players"]').count(), 1, 'Original Players sidebar route missing');
    assert.equal(await page.locator('a[href="#/teams"]').count(), 1, 'Original Teams sidebar route missing');
    assert.deepEqual(pageErrors, [], `Renderer errors: ${pageErrors.join('; ')}`);
    const screenshot = path.join(outputDir, 'original-matches-empty.png');
    await page.screenshot({ path: screenshot, animations: 'disabled' });
    assert.ok(fs.statSync(screenshot).size > 10000, 'Native screenshot is unexpectedly small');
    const proof = {
      source: 'actual hosted Tauri WebView2 window with original CS Demo Manager renderer',
      data: 'new persistent local SQLite library with no imported matches',
      scope: 'startup and empty Matches route only; import and other route backends remain unported',
      commit: process.env.GITHUB_SHA || null,
      executableSha256: sha256(appPath),
      screenshotSha256: sha256(screenshot),
      runtimeUrl: page.url(),
      originalSidebarRoutes: ['matches', 'demos', 'players', 'teams'],
    };
    fs.writeFileSync(path.join(outputDir, 'proof.json'), `${JSON.stringify(proof, null, 2)}\n`);
    console.log('PASS: original renderer startup and empty Matches route in native Tauri WebView2');
  } catch (error) {
    if (page) {
      const screenshot = path.join(outputDir, 'original-startup-failure.png');
      await page.screenshot({ path: screenshot, animations: 'disabled' }).catch(() => {});
      const visible = await page.evaluate(() => ({
        url: location.href,
        title: document.title,
        text: document.body.innerText.slice(0, 3000),
      })).catch((inspectionError) => ({ inspectionError: String(inspectionError) }));
      fs.writeFileSync(path.join(outputDir, 'failure.json'), `${JSON.stringify({
        source: 'hosted fresh-profile Tauri WebView2 window',
        error: String(error),
        visible,
      }, null, 2)}\n`);
      console.error('Native app visible state:', JSON.stringify(visible));
    }
    throw error;
  } finally {
    if (browser) await browser.close().catch(() => {});
    if (exit === null) app.kill();
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
