// Hosted Windows only: exercise a pinned public CS2 demo in the actual Tauri
// WebView2 window. In the feature-free build, deliberately set the synthetic
// parser environment variable and prove the native decoder still runs.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { chromium } = require('playwright-core');

const outputDir = path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'demostrike-public-native');
const demoPath = path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'public-cs2-fixture.dem');
const appPath = path.resolve(__dirname, '../src-tauri/target/release/demostrike-rust-candidate.exe');
const cdpUrl = 'http://127.0.0.1:19375';
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');

async function captureAppOnly(page, file) {
  await page.screenshot({ path: file, animations: 'disabled' });
  // Small previews from the same actual WebView2 window let review inspect
  // pixels through hosted logs when artifact downloads are unavailable.
  const preview = await page.screenshot({ type: 'jpeg', quality: 42, animations: 'disabled' });
  const label = path.basename(file, '.png');
  const encoded = preview.toString('base64');
  console.log(`APP_PREVIEW_BEGIN ${label} ${preview.length}`);
  for (let offset = 0; offset < encoded.length; offset += 4096) {
    console.log(`APP_PREVIEW_CHUNK ${label} ${offset / 4096} ${encoded.slice(offset, offset + 4096)}`);
  }
  console.log(`APP_PREVIEW_END ${label}`);
}

async function main() {
  assert.equal(process.platform, 'win32');
  assert.ok(fs.existsSync(appPath), `Native app missing: ${appPath}`);
  assert.equal(fs.statSync(demoPath).size, 60601900);
  const sourceHash = sha256(demoPath);
  fs.mkdirSync(outputDir, { recursive: true });
  let portFree = false;
  for (let attempt = 0; attempt < 30; attempt += 1) {
    try {
      const old = await fetch(`${cdpUrl}/json/version`, { signal: AbortSignal.timeout(500) });
      if (old.ok) { await sleep(500); continue; }
      portFree = true;
      break;
    } catch { portFree = true; break; }
  }
  assert.ok(portFree, 'Prior native app still owns the WebView2 debugging endpoint');
  const env = { ...process.env };
  const featureFree = process.env.EXPECT_FEATURE_FREE === '1';
  if (featureFree) {
    // Deliberately request synthetic facts. A release build without the
    // feature must ignore this environment variable and decode the real demo.
    env.DEMOSTRIKE_SYNTHETIC_RAW_FACTS = path.join(__dirname, 'fixtures', 'combat-raw.json');
    assert.ok(fs.existsSync(env.DEMOSTRIKE_SYNTHETIC_RAW_FACTS));
  } else {
    delete env.DEMOSTRIKE_SYNTHETIC_RAW_FACTS;
  }
  const app = spawn(appPath, [], { cwd: path.dirname(appPath), windowsHide: false, stdio: 'pipe', env });
  let exit = null;
  app.once('exit', (code, signal) => { exit = `${code ?? 'none'} / ${signal ?? 'none'}`; });
  app.stdout.on('data', (data) => process.stdout.write(data));
  app.stderr.on('data', (data) => process.stderr.write(data));
  let browser;
  let page;
  const pageErrors = [];
  try {
    let ready = false;
    for (let attempt = 0; attempt < 90; attempt += 1) {
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
    await page.evaluate(() => { location.hash = '#/matches'; });
    await page.waitForFunction(() => Boolean(document.querySelector('#demo-import-path'))
      || [...document.querySelectorAll('button')].some((button) => button.textContent?.includes('Change library')),
      null, { timeout: 30000 });
    const changeLibrary = page.getByRole('button', { name: 'Change library' });
    if (await changeLibrary.isVisible().catch(() => false)) await changeLibrary.click();
    await page.getByRole('textbox', { name: 'Import a CS2 demo' }).fill(demoPath);
    await page.getByRole('button', { name: 'Import demo' }).click();
    const row = page.getByRole('row').filter({ hasText: 'public-cs2-fixture.dem' });
    await row.waitFor({ timeout: 180000 });
    assert.ok((await row.innerText()).includes('de_mirage'));
    const matchesShot = path.join(outputDir, 'real-demo-matches.png');
    await captureAppOnly(page, matchesShot);
    await row.dblclick();
    await page.getByRole('heading', { name: 'de_mirage' }).waitFor({ timeout: 30000 });
    await page.getByRole('heading', { name: 'Recorded players' }).waitFor({ timeout: 30000 });
    assert.equal(await page.getByText('Synthetic CI fixture:', { exact: false }).count(), 0);
    const capability = await page.evaluate(async () => {
      const library = localStorage.getItem('csdm-port-selected-library');
      const matchId = Number(location.hash.match(/local-(\d+)/)?.[1]);
      if (!library || !Number.isSafeInteger(matchId)) throw new Error('Local match route not available');
      const detail = await window.__TAURI_INTERNALS__.invoke('get_match_detail', { path: library, matchId });
      return detail.summary.parserCapability;
    });
    assert.equal(capability, 'native-combat-full-v2', 'Real decoder provenance was not retained');
    const overviewShot = path.join(outputDir, 'real-demo-overview.png');
    await captureAppOnly(page, overviewShot);
    await page.getByRole('link', { name: 'Rounds', exact: true }).click();
    await page.getByRole('heading', { name: 'Recorded rounds' }).waitFor({ timeout: 30000 });
    await page.waitForFunction(() => document.querySelectorAll('section[aria-label="Round entries"] details').length >= 10, null, { timeout: 30000 });
    const roundRows = await page.locator('section[aria-label="Round entries"] details').count();
    assert.ok(roundRows >= 10, `Unexpected round count: ${roundRows}`);
    const roundsShot = path.join(outputDir, 'real-demo-rounds.png');
    await captureAppOnly(page, roundsShot);
    await page.locator('section[aria-label="Round entries"] details').first().locator('summary').click();
    await page.getByRole('heading', { name: 'Recorded death events' }).first().waitFor({ timeout: 30000 });
    await page.getByText('All CT eliminated').first().waitFor({ timeout: 30000 });
    assert.ok(await page.locator('section[aria-label="Round entries"] ol li').count() > 0,
      'Native raw death events did not reach the partial CSDM round view');
    const roundDetailShot = path.join(outputDir, 'real-demo-round-detail.png');
    await captureAppOnly(page, roundDetailShot);
    await page.getByRole('link', { name: 'Players' }).click();
    await page.getByRole('heading', { name: 'Recorded players' }).waitFor({ timeout: 30000 });
    await page.waitForFunction(() => document.querySelectorAll('tbody tr').length >= 10, null, { timeout: 30000 });
    const playerRows = await page.locator('tbody tr').count();
    assert.ok(playerRows >= 10, `Unexpected player count: ${playerRows}`);
    const playersShot = path.join(outputDir, 'real-demo-players.png');
    await captureAppOnly(page, playersShot);
    const firstPlayerLink = page.locator('[aria-label="Recorded player scoreboard"] tbody tr').first().getByRole('link').first();
    const firstPlayerName = (await firstPlayerLink.innerText()).trim();
    await firstPlayerLink.click();
    await page.getByRole('heading', { name: firstPlayerName, exact: true }).waitFor({ timeout: 30000 });
    await page.getByRole('heading', { name: 'Round by round' }).waitFor({ timeout: 30000 });
    const playerDetailRows = await page.locator('section[aria-label="Player rounds"] tbody tr').count();
    assert.equal(playerDetailRows, 9, 'Only nine live rounds have recorded player facts; the final surrender must stay unavailable');
    const playerDetailShot = path.join(outputDir, 'real-demo-player-detail.png');
    await captureAppOnly(page, playerDetailShot);
    await page.getByRole('link', { name: 'All players' }).click();
    await page.getByRole('heading', { name: 'Recorded players' }).waitFor({ timeout: 30000 });
    await page.getByRole('link', { name: 'Utility' }).click();
    await page.getByRole('heading', { name: 'Recorded utility' }).waitFor({ timeout: 30000 });
    await page.waitForFunction(() => document.querySelectorAll('tbody tr').length === 150, null, { timeout: 30000 });
    const utilityRows = await page.locator('tbody tr').count();
    assert.equal(utilityRows, 150);
    const utilityShot = path.join(outputDir, 'real-demo-utility.png');
    await captureAppOnly(page, utilityShot);
    // Exercise failure recovery in the same native window. Neither attempt
    // may replace or duplicate the successfully imported match.
    await page.evaluate(() => { location.hash = '#/matches'; });
    await page.getByRole('button', { name: 'Import demo' }).click();
    const importPath = page.getByRole('textbox', { name: 'Import a CS2 demo' });
    await importPath.fill(demoPath);
    await page.getByRole('button', { name: 'Import demo' }).last().click();
    await page.getByRole('alert').filter({ hasText: 'already imported' }).waitFor({ timeout: 30000 });
    const duplicateShot = path.join(outputDir, 'duplicate-demo-error.png');
    await captureAppOnly(page, duplicateShot);
    const corruptPath = path.join(outputDir, 'invalid-demo.dem');
    fs.writeFileSync(corruptPath, 'not a valid CS2 demo');
    await importPath.fill(corruptPath);
    await page.getByRole('button', { name: 'Import demo' }).last().click();
    await page.getByRole('alert').filter({ hasText: 'not a CS2 PBDEMS2 demo' }).waitFor({ timeout: 30000 });
    const invalidShot = path.join(outputDir, 'invalid-demo-error.png');
    await captureAppOnly(page, invalidShot);
    assert.equal(await page.getByRole('row').filter({ hasText: 'public-cs2-fixture.dem' }).count(), 1);
    assert.equal(sha256(demoPath), sourceHash, 'Native import changed public demo bytes');
    assert.deepEqual(pageErrors, [], `Renderer errors: ${pageErrors.join('; ')}`);
    const proof = {
      source: 'actual hosted Tauri WebView2 window using the native source2-demo decoder',
      fixture: 'LaihoE/demoparser@45ca85ae:src/parser/test_demo.dem',
      fixtureBytes: fs.statSync(demoPath).size,
      fixtureSha256: sourceHash,
      commit: process.env.GITHUB_SHA || null,
      executableSha256: sha256(appPath),
      featureFreeReleaseBuild: featureFree,
      syntheticEnvironmentDeliberatelySet: featureFree,
      parserCapability: capability,
      observed: { roundRows, playerRows, playerDetailRows, utilityRows },
      screenshots: Object.fromEntries([matchesShot, overviewShot, roundsShot, roundDetailShot, playersShot, playerDetailShot, utilityShot, duplicateShot, invalidShot]
        .map((file) => [path.basename(file), sha256(file)])),
      runtimeUrl: page.url(),
      scope: 'real demo import and data-limited match views; full upstream CSDM match contracts remain unported',
    };
    fs.writeFileSync(path.join(outputDir, 'proof.json'), `${JSON.stringify(proof, null, 2)}\n`);
    console.log('PASS: public CS2 demo imported and rendered in native Tauri WebView2');
  } catch (error) {
    if (page) {
      await page.screenshot({ path: path.join(outputDir, 'real-demo-failure.png'), animations: 'disabled' }).catch(() => {});
      const visible = await page.evaluate(() => ({ url: location.href, text: document.body.innerText.slice(0, 3000) }))
        .catch((inspectionError) => ({ inspectionError: String(inspectionError) }));
      fs.writeFileSync(path.join(outputDir, 'failure.json'), `${JSON.stringify({ error: String(error), pageErrors, visible }, null, 2)}\n`);
      console.error('Native app visible state:', JSON.stringify({ visible, pageErrors }));
    }
    throw error;
  } finally {
    if (browser) await browser.close().catch(() => {});
    if (exit === null) app.kill();
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
