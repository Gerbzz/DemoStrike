// Hosted Windows regression: an explicit import after Change library retains
// its intent when the native process is killed before the receipt opens.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { chromium } = require('playwright-core');

const appPath = path.resolve(__dirname, '../src-tauri/target/release/demostrike-rust-candidate.exe');
const outputDir = path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'demostrike-original');
const demoPath = path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'demostrike-smoke', 'synthetic-restart-intent.dem');
const cdpUrl = 'http://127.0.0.1:19375';
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');

async function launch(parseWaitMs) {
  const app = spawn(appPath, [], {
    cwd: path.dirname(appPath), windowsHide: false, stdio: 'pipe',
    env: { ...process.env, DEMOSTRIKE_SYNTHETIC_PARSE_WAIT_MS: String(parseWaitMs) },
  });
  let exited = false;
  app.once('exit', () => { exited = true; });
  app.stdout.on('data', (data) => process.stdout.write(data));
  app.stderr.on('data', (data) => process.stderr.write(data));
  try {
    let ready = false;
    for (let attempt = 0; attempt < 60; attempt += 1) {
      if (exited) throw new Error('Native app exited before WebView2 opened');
      try {
        const response = await fetch(`${cdpUrl}/json/version`, { signal: AbortSignal.timeout(1500) });
        if (response.ok) { ready = true; break; }
      } catch { /* Startup is still in progress. */ }
      await sleep(1000);
    }
    assert.ok(ready, 'WebView2 CDP endpoint did not open');
    const browser = await chromium.connectOverCDP(cdpUrl, { timeout: 15000 });
    let page;
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
    assert.ok(page, 'No IPC-enabled native page appeared');
    return { app, browser, page, isExited: () => exited };
  } catch (error) {
    if (!exited) app.kill();
    throw error;
  }
}

async function close(session) {
  await session.browser.close().catch(() => {});
  if (!session.isExited()) session.app.kill();
  for (let attempt = 0; attempt < 30 && !session.isExited(); attempt += 1) await sleep(100);
  assert.ok(session.isExited(), 'Task-owned native process did not stop');
}

async function findJob(page) {
  return page.evaluate(async (source) => {
    const jobs = await window.__TAURI_INTERNALS__.invoke('csdm_list_import_jobs');
    return jobs.find((job) => job.demoPath === source) ?? null;
  }, demoPath);
}

async function main() {
  assert.equal(process.platform, 'win32');
  fs.mkdirSync(path.dirname(demoPath), { recursive: true });
  fs.mkdirSync(outputDir, { recursive: true });
  fs.writeFileSync(demoPath, Buffer.from('PBDEMS2\0synthetic restart intent fixture'));
  const sourceHash = sha256(demoPath);
  let first;
  let second;
  try {
    first = await launch(12000);
    await first.page.locator('a[href="#/matches"]').click();
    await first.page.getByRole('button', { name: 'Change library' }).click();
    await first.page.getByText('Your local library is empty').waitFor({ timeout: 20000 });
    assert.equal(await first.page.evaluate(() => localStorage.getItem('csdm-port-manual-library-change')), '1');
    await first.page.getByRole('textbox', { name: 'Import a CS2 demo' }).fill(demoPath);
    await first.page.getByRole('button', { name: 'Import demo' }).click();
    await first.page.getByText('Analyzing demo in the import queue.', { exact: false }).waitFor({ timeout: 20000 });
    const started = await findJob(first.page);
    assert.equal(started?.state, 'running', 'Import did not reach the running state before interruption');
    assert.equal(await first.page.evaluate(() => localStorage.getItem('csdm-port-intended-import-job-id')),
      String(started.id), 'The new user import intent was not persisted');
    await close(first);
    first = null;

    second = await launch(100);
    await second.page.locator('a[href="#/matches"]').click();
    const recoveredRow = second.page.getByRole('row').filter({ hasText: 'synthetic-restart-intent.dem' });
    await recoveredRow.waitFor({ timeout: 45000 });
    const finished = await findJob(second.page);
    assert.equal(finished?.id, started.id, 'Restart created a different queue job');
    assert.equal(finished?.state, 'completed', 'Recovered import did not complete');
    assert.ok(finished.receipt?.path, 'Recovered import has no saved receipt');
    assert.equal(await second.page.evaluate(() => localStorage.getItem('csdm-port-selected-library')),
      finished.receipt.path, 'Saved receipt did not open after process restart');
    assert.equal(sha256(demoPath), sourceHash, 'Recovery changed the source demo');
    const screenshot = path.join(outputDir, 'recovered-explicit-import-after-restart.png');
    await second.page.screenshot({ path: screenshot, animations: 'disabled' });
    const jpeg = await second.page.screenshot({ type: 'jpeg', quality: 45, animations: 'disabled' });
    const encoded = jpeg.toString('base64');
    console.log(`APP_PREVIEW_BEGIN recovered-explicit-import-after-restart ${jpeg.length}`);
    for (let offset = 0; offset < encoded.length; offset += 4096) {
      console.log(`APP_PREVIEW_CHUNK recovered-explicit-import-after-restart ${offset / 4096} ${encoded.slice(offset, offset + 4096)}`);
    }
    console.log(`APP_PREVIEW_END recovered-explicit-import-after-restart ${crypto.createHash('sha256').update(jpeg).digest('hex')}`);
    fs.writeFileSync(path.join(outputDir, 'recovered-intent-proof.json'), JSON.stringify({
      source: 'actual hosted Tauri WebView2 process interruption and restart',
      data: 'synthetic parser facts and real durable queue/library',
      jobId: started.id,
      receipt: finished.receipt,
      sourceSha256: sourceHash,
      screenshotSha256: sha256(screenshot),
      commit: process.env.GITHUB_SHA || null,
    }, null, 2) + '\n');
    console.log('PASS: explicit import intent survived process crash and opened its recovered receipt');
  } finally {
    if (first) await close(first);
    if (second) await close(second);
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });

