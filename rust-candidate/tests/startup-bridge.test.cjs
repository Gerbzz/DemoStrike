const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const { stripTypeScriptTypes } = require('node:module');

function loadBridge(invoke) {
  const bridgePath = join(__dirname, '../ported/preload-bridge.ts');
  const source = readFileSync(bridgePath, 'utf8')
    .replace(/^import .*;\r?\n/gm, '');
  const window = { __TAURI_INTERNALS__: { invoke } };
  runInNewContext(stripTypeScriptTypes(source), {
    window,
    navigator: { userAgent: 'Windows' },
    console,
    defaultSettings: { ui: { theme: 'dark' } },
    mergeSettings: () => { throw new Error('not used'); },
  }, { filename: bridgePath });
  return window.csdm;
}

test('startup bridge forwards get and clear through the existing native request', async () => {
  const calls = [];
  const startup = [{ name: 'start-path', value: 'demos' }];
  let current = startup;
  const bridge = loadBridge(async (command, args) => {
    calls.push([command, args.name]);
    if (args.name === 'get-startup-arguments') return current;
    if (args.name === 'clear-startup-arguments') { current = []; return null; }
    throw new Error('unexpected request');
  });
  assert.equal(await bridge.getStartupArguments(), startup);
  assert.equal(await bridge.clearStartupArguments(), undefined);
  assert.equal((await bridge.getStartupArguments()).length, 0);
  assert.deepEqual(calls, [
    ['csdm_request', 'get-startup-arguments'],
    ['csdm_request', 'clear-startup-arguments'],
    ['csdm_request', 'get-startup-arguments'],
  ]);
});

test('startup bridge propagates native errors instead of returning empty arguments', async () => {
  const bridge = loadBridge(async () => { throw new Error('startup unavailable'); });
  await assert.rejects(bridge.getStartupArguments(), /startup unavailable/);
  await assert.rejects(bridge.clearStartupArguments(), /startup unavailable/);
});
