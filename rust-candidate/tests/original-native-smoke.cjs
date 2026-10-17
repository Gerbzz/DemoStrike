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
const fixturePath = path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'demostrike-smoke', 'synthetic-library.sqlite');
const appPath = path.resolve(__dirname, '../src-tauri/target/release/demostrike-rust-candidate.exe');
const cdpUrl = 'http://127.0.0.1:19375';
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');

async function main() {
  assert.equal(process.platform, 'win32');
  assert.ok(fs.existsSync(appPath), `Native app missing: ${appPath}`);
  assert.ok(fs.existsSync(fixturePath), `Synthetic library missing: ${fixturePath}`);
  const fixtureSha256 = sha256(fixturePath);
  fs.mkdirSync(outputDir, { recursive: true });
  const app = spawn(appPath, [], { cwd: path.dirname(appPath), windowsHide: false, stdio: 'pipe' });
  let exit = null;
  app.once('exit', (code, signal) => { exit = `${code ?? 'none'} / ${signal ?? 'none'}`; });
  app.stdout.on('data', (data) => process.stdout.write(data));
  app.stderr.on('data', (data) => process.stderr.write(data));
  let browser;
  let page;
  const pageErrors = [];
  const consoleErrors = [];
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
    page.on('pageerror', (error) => pageErrors.push(error.message));
    page.on('console', (message) => {
      if (message.type() === 'error') consoleErrors.push(message.text());
    });
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
    await page.getByRole('textbox', { name: 'Open an existing DemoStrike SQLite library' }).fill(fixturePath);
    await page.getByRole('button', { name: 'Open local library' }).click();
    await page.getByText('2 recorded matches').waitFor({ timeout: 20000 });
    const firstMatch = page.getByRole('row').filter({ hasText: 'synthetic-alpha.dem' });
    assert.equal(await firstMatch.count(), 1, 'First synthetic match missing from original table engine');
    const secondMatch = page.getByRole('row').filter({ hasText: 'synthetic-beta.dem' });
    assert.equal(await secondMatch.count(), 1);
    assert.ok((await secondMatch.innerText()).includes('—'), 'Unknown duration must not appear as zero');
    const matchesScreenshot = path.join(outputDir, 'ported-populated-matches.png');
    await page.screenshot({ path: matchesScreenshot, animations: 'disabled' });
    await firstMatch.dblclick();
    await page.getByRole('heading', { name: 'de_mirage' }).waitFor({ timeout: 20000 });
    assert.ok((await page.getByText('Team identities, kills, assists, ratings, and economy are unavailable').count()) > 0);
    const overviewScreenshot = path.join(outputDir, 'ported-match-overview.png');
    await page.screenshot({ path: overviewScreenshot, animations: 'disabled' });
    await page.getByRole('link', { name: 'Rounds' }).click();
    await page.getByRole('heading', { name: 'Recorded rounds' }).waitFor({ timeout: 20000 });
    assert.equal(await page.locator('tbody tr').count(), 3);
    assert.ok((await page.locator('tbody').innerText()).includes('Unavailable'));
    const roundsScreenshot = path.join(outputDir, 'ported-match-rounds.png');
    await page.screenshot({ path: roundsScreenshot, animations: 'disabled' });
    await page.getByRole('link', { name: 'Players' }).click();
    await page.getByRole('heading', { name: 'Recorded players' }).waitFor({ timeout: 20000 });
    assert.equal(await page.locator('tbody tr').count(), 2);
    const playersScreenshot = path.join(outputDir, 'ported-match-players.png');
    await page.screenshot({ path: playersScreenshot, animations: 'disabled' });
    await page.getByRole('link', { name: 'Utility' }).click();
    await page.getByRole('heading', { name: 'Recorded utility' }).waitFor({ timeout: 20000 });
    assert.equal(await page.locator('tbody tr').count(), 3);
    assert.ok((await page.locator('tbody').innerText()).includes('Unavailable'));
    const utilityScreenshot = path.join(outputDir, 'ported-match-utility.png');
    await page.screenshot({ path: utilityScreenshot, animations: 'disabled' });
    await page.evaluate(() => { location.hash = '#/matches/local-101/heatmap'; });
    await page.getByRole('heading', { name: 'This match analysis is still being ported' }).waitFor({ timeout: 20000 });
    const unsupportedScreenshot = path.join(outputDir, 'ported-match-unsupported.png');
    await page.screenshot({ path: unsupportedScreenshot, animations: 'disabled' });
    assert.equal(sha256(fixturePath), fixtureSha256, 'Read-only viewer changed the synthetic fixture');
    const demoFolder = path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'demostrike-synthetic-demos');
    fs.mkdirSync(demoFolder, { recursive: true });
    const alphaDemo = path.join(demoFolder, 'synthetic-alpha.dem');
    const betaDemo = path.join(demoFolder, 'synthetic-beta.DEM');
    fs.writeFileSync(alphaDemo, 'synthetic test file one');
    fs.writeFileSync(betaDemo, 'synthetic test file two');
    fs.writeFileSync(path.join(demoFolder, 'not-a-demo.txt'), 'ignored');
    const alphaHash = sha256(alphaDemo);
    const betaHash = sha256(betaDemo);
    await page.locator('a[href="#/demos"]').click();
    await page.getByRole('heading', { name: 'Demo files' }).waitFor({ timeout: 20000 });
    await page.getByRole('textbox', { name: 'Demo folder' }).fill(demoFolder);
    await page.getByRole('button', { name: 'Open folder' }).click();
    await page.getByText('2 demo files').waitFor({ timeout: 20000 });
    assert.equal(await page.getByRole('cell', { name: 'synthetic-alpha.dem' }).count(), 1);
    assert.equal(await page.getByRole('cell', { name: 'synthetic-beta.DEM' }).count(), 1);
    assert.equal(await page.getByRole('cell', { name: 'not-a-demo.txt' }).count(), 0);
    await page.getByRole('textbox', { name: 'Find demo' }).fill('beta');
    assert.equal(await page.getByRole('cell', { name: 'synthetic-alpha.dem' }).count(), 0);
    await page.getByRole('textbox', { name: 'Find demo' }).fill('');
    const demoScreenshot = path.join(outputDir, 'ported-demo-folder.png');
    await page.screenshot({ path: demoScreenshot, animations: 'disabled' });
    assert.equal(sha256(alphaDemo), alphaHash, 'Read-only browser changed a synthetic demo file');
    assert.equal(sha256(betaDemo), betaHash, 'Read-only browser changed a synthetic demo file');
    const proof = {
      source: 'actual hosted Tauri WebView2 window with original CS Demo Manager renderer',
      data: 'new empty local SQLite plus a separate sanitized synthetic DemoStrike library',
      scope: 'original CSDM shell and table engine with data-limited match, rounds, players, and demo-folder routes; import and other backend contracts remain unported',
      commit: process.env.GITHUB_SHA || null,
      executableSha256: sha256(appPath),
      screenshotSha256: sha256(screenshot),
      matchesScreenshotSha256: sha256(matchesScreenshot),
      overviewScreenshotSha256: sha256(overviewScreenshot),
      roundsScreenshotSha256: sha256(roundsScreenshot),
      playersScreenshotSha256: sha256(playersScreenshot),
      utilityScreenshotSha256: sha256(utilityScreenshot),
      unsupportedScreenshotSha256: sha256(unsupportedScreenshot),
      demoFolderScreenshotSha256: sha256(demoScreenshot),
      syntheticDemoHashes: [alphaHash, betaHash],
      fixtureSha256,
      runtimeUrl: page.url(),
      originalSidebarRoutes: ['matches', 'demos', 'players', 'teams'],
    };
    fs.writeFileSync(path.join(outputDir, 'proof.json'), `${JSON.stringify(proof, null, 2)}\n`);
    assert.deepEqual(pageErrors, [], `Renderer errors: ${pageErrors.join('; ')}`);
    console.log('PASS: original renderer routes and populated read-only match workflow in native Tauri WebView2');
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
        pageErrors,
        consoleErrors,
        visible,
      }, null, 2)}\n`);
      console.error('Native app visible state:', JSON.stringify({ visible, pageErrors, consoleErrors }));
    }
    throw error;
  } finally {
    if (browser) await browser.close().catch(() => {});
    if (exit === null) app.kill();
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
