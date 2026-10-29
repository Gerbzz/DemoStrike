const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const { stripTypeScriptTypes } = require('node:module');

function harness({ startPath, defaultPage = 'matches', navigate } = {}) {
  const file = process.env.DEMOSTRIKE_REDIRECTOR_SOURCE || join(__dirname, '../ported/initial-route-redirector.ts');
  const source = readFileSync(file, 'utf8').replace(/^import .*;\r?\n/gm, '').replace('export function InitialRouteRedirector', 'function InitialRouteRedirector');
  const ref = { current: false };
  let effect;
  const calls = [];
  let settingsOpens = 0;
  const state = { startPath, defaultPage };
  const context = {
    useEffect: (callback) => { effect = callback; },
    useRef: () => ref,
    useUiSettings: () => ({ initialPage: state.defaultPage }),
    useSettingsOverlay: () => ({ openSettings: () => { settingsOpens++; } }),
    useArgument: () => state.startPath,
    useNavigate: () => (path, options) => { calls.push([path, options.replace]); return navigate ? navigate(path) : Promise.resolve(); },
    ArgumentName: { StartPath: 'start-path' },
    StartPath: { Bans:'bans', Demos:'demos', Downloads:'downloads', Matches:'matches', Players:'players', Search:'search', Settings:'settings', Teams:'teams' },
    Page: { Demos:'demos', Download:'download', Players:'players', Teams:'teams' },
    RoutePath: { Matches:'/matches', Demos:'/demos', Downloads:'/downloads', Players:'/players', Teams:'/teams', Search:'/search', Ban:'/ban' },
    buildPendingDownloadPath: () => '/downloads/pending',
  };
  runInNewContext(stripTypeScriptTypes(source) + '\nglobalThis.redirector = InitialRouteRedirector;', context, { filename: file });
  return { state, calls, ref, get settingsOpens() { return settingsOpens; }, render() { context.redirector(); effect(); } };
}

if (!process.env.STARTUP_REDIRECTOR_CHILD) {
test('clearing the startup argument while navigation is pending does not redirect twice', () => {
  const pending = new Promise(() => {});
  const h = harness({ startPath:'demos', navigate: () => pending });
  h.render();
  h.state.startPath = undefined;
  h.render();
  assert.deepEqual(h.calls, [['/demos', true]]);
});

test('startup and default routes preserve original mappings', async () => {
  for (const [arg, defaultPage, expected, opens] of [
    ['demos','matches','/demos',0], ['downloads','matches','/downloads/pending',0],
    ['settings','teams','/teams',1], ['', 'demos','/demos',0],
    [undefined,'players','/players',0], ['unknown','demos','/matches',0],
  ]) {
    const h = harness({ startPath:arg, defaultPage });
    h.render();
    assert.deepEqual(h.calls, [[expected,true]]);
    assert.equal(h.settingsOpens, opens);
    await Promise.resolve();
    h.render();
    assert.equal(h.calls.length, 1);
  }
});


test('a rejected navigation resets the guard and propagates the error', () => {
  const { spawnSync } = require('node:child_process');
  const child = `
    const assert = require('node:assert/strict');
    const { harness } = require(process.argv[1]);
    let attempt = 0;
    const h = harness({ startPath: 'demos', navigate: () => ++attempt === 1 ? Promise.reject(new Error('navigation rejected')) : Promise.resolve() });
    let sawError = false;
    process.on('unhandledRejection', (error) => {
      assert.equal(error.message, 'navigation rejected');
      assert.equal(h.ref.current, false);
      sawError = true;
      h.render();
      assert.equal(h.calls.length, 2);
    });
    h.render();
    setTimeout(() => { if (!sawError) throw new Error('Expected navigation rejection was not propagated'); }, 100);
  `;
  const result = spawnSync(process.execPath, ['-e', child, __filename], {
    env: { ...process.env, STARTUP_REDIRECTOR_CHILD: '1' }, encoding: 'utf8', timeout: 5000,
  });
  assert.equal(result.status, 0, result.stderr || result.error?.message);
});
}
module.exports = { harness };
