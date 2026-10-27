// Execute the actual native viewer adapter and pinned CSDM bomb drawing hook.
// Only React mounting and canvas pixels are substituted: the event selection,
// context gate, and drawing implementation come from the checked-out source.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');

const root = path.resolve(__dirname, '..');
const provider = Symbol('ViewerContext.Provider');
let currentContext;
const react = {
  createElement: (type, props, ...children) => ({ type, props: props ?? {}, children }),
  useMemo: (build) => build(),
  useState: (initial) => [initial, () => {}],
  useEffect: () => {},
  useRef: (initial) => ({ current: initial }),
};
const sides = { UNASSIGNED: 0, T: 2, CT: 3 };
const cache = new Map();
function load(file) {
  const absolute = path.resolve(root, file);
  if (cache.has(absolute)) return cache.get(absolute).exports;
  const module = { exports: {} };
  cache.set(absolute, module);
  let source = fs.readFileSync(absolute, 'utf8');
  if (file === 'ported/native-original-viewer-2d.tsx') {
    source += '\nexport { NativeOriginalViewer };\n';
  }
  const compiled = ts.transpileModule(source, { fileName: absolute, compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022,
    jsx: ts.JsxEmit.React, esModuleInterop: true,
  } }).outputText;
  const requireSource = (specifier) => {
    if (specifier === 'react') return react;
    if (specifier === 'react-router') return { useParams: () => ({ checksum: 'local-3' }), useNavigate: () => () => {} };
    if (specifier === 'csdm/common/types/counter-strike') return {
      TeamNumber: sides, Game: { CS2: 'cs2' }, WeaponName: { World: 'world' }, GrenadeName: {},
    };
    if (specifier.endsWith('/use-viewer-context')) return { useViewerContext: () => currentContext };
    if (specifier.endsWith('/viewer-context')) return { ViewerContext: { Provider: provider } };
    if (specifier.endsWith('/viewer-2d')) return { Viewer2D: () => {} };
    if (specifier === './native-original-viewer-events') return load('ported/native-original-viewer-events.ts');
    if (specifier === './native-original-viewer-match') return load('ported/native-original-viewer-match.ts');
    if (specifier === './get-bomb-image') return load('upstream/cs-demo-manager/src/ui/match/viewer-2d/drawing/get-bomb-image.ts');
    if (specifier === './native-original-viewer-radar') return { NativeRadarProvider: () => {} };
    if (specifier === './native-original-event-list') return { NativeOriginalEventList: () => {} };
    if (['./match-data', './match-loader', './native-selected-radar', './native-original-viewer.css'].includes(specifier)) return {};
    throw new Error(`Unexpected runtime import ${specifier} in ${file}`);
  };
  vm.runInNewContext(compiled, {
    exports: module.exports, module, require: requireSource,
    Image: class { src = ''; },
  }, { filename: absolute });
  return module.exports;
}

const { NativeOriginalViewer } = load('ported/native-original-viewer-2d.tsx');
const { useDrawBombs } = load('upstream/cs-demo-manager/src/ui/match/viewer-2d/drawing/use-draw-bombs.ts');
const plantTick = 21853;
const plant = { kind: 'bomb_planted', tick: plantTick, sourceIndex: 1,
  actorSteamid: 'player-1', actorName: 'Player', x: 10, y: 20, z: 30 };
const detail = { players: [], rounds: [], summary: {} };
const recorded = { roundNumber: 3, startTick: 17000, freezeEndTick: 18000, endTick: 23000,
  deaths: [], teamASide: 'CT', teamBSide: 'T', winner: null };
const baseMeta = { mapName: 'de_mirage', roundNumber: 3, startTick: 17000, endTick: 23000,
  tickRate: 64, utilityEvents: [], bombEvents: [plant], bombEventsAvailable: true,
  observedBombSelection: { plantIndex: 0, explosionIndex: null, defusalIndex: null } };

function contextAt(tick, meta = baseMeta) {
  const tree = NativeOriginalViewer({ detail, recorded, meta, initialTick: tick, radarBitmap: null,
    frames: { grenades: [], players: [{ tick, steamid: 'player-1', name: 'Player',
      side: 'CT', x: 1, y: 2, z: 3, yaw: 0, health: 100, isAlive: true }] } });
  const contextNode = tree.children.find((child) => child.type === provider);
  assert.ok(contextNode, 'Mounted native viewer did not supply the pinned viewer context');
  return contextNode.props.value;
}

function draw(context) {
  currentContext = context;
  let historySearches = 0;
  context.playerPositions.find = () => {
    historySearches++;
    throw new Error('Entered unavailable bomb-carrier history search before the observed plant');
  };
  const calls = [];
  const canvas = { globalAlpha: 1, save() {}, restore() {},
    drawImage(image, x, y, width, height) { calls.push({ image, x, y, width, height, alpha: this.globalAlpha }); } };
  const hook = useDrawBombs();
  // The retained upstream Viewer2D uses exactly this gate around drawBombs.
  if (context.shouldDrawBombs) hook.drawBombs(canvas, {
    zoomedToRadarX: (x) => x, zoomedToRadarY: (y) => y, zoomedSize: (size) => size,
  });
  assert.equal(historySearches, 0, 'Unrecorded carrier state triggered an absolute-tick history scan');
  return calls;
}

const viewerSource = fs.readFileSync(path.join(root, 'upstream/cs-demo-manager/src/ui/match/viewer-2d/viewer-2d.tsx'), 'utf8');
assert.match(viewerSource, /if \(shouldDrawBombs\)\s*\{\s*drawBombs\(context, interactiveCanvas\)/,
  'Pinned Viewer2D drawing gate changed; update the integration regression');

// Large absolute ticks are deliberate: the old gate entered a backwards scan
// from 21852 to zero for every animation frame with no observable bomb carrier.
const before = contextAt(plantTick - 1);
assert.equal(before.playerPositions[0].hasBomb, false, 'Native carrier state must remain unavailable');
assert.equal(before.bombPlanted.tick, plantTick, 'Pre-plant timeline marker was discarded');
assert.equal(draw(before).length, 0, 'A bomb was invented before the observed plant');
assert.equal(before.shouldDrawBombs, false);

const planted = contextAt(plantTick);
assert.equal(planted.shouldDrawBombs, true, 'Exact observed plant tick stopped drawing');
const plantedDraw = draw(planted);
assert.equal(plantedDraw.length, 1);
assert.deepEqual({ x: plantedDraw[0].x, y: plantedDraw[0].y, width: plantedDraw[0].width, alpha: plantedDraw[0].alpha },
  { x: -1, y: 9, width: 22, alpha: 1 }, 'Pinned planted bomb coordinates or size changed');

const defused = contextAt(21900, { ...baseMeta,
  bombEvents: [plant, { ...plant, kind: 'bomb_defused', tick: 21900, sourceIndex: 2 }],
  observedBombSelection: { plantIndex: 0, explosionIndex: null, defusalIndex: 1 } });
assert.equal(defused.bombDefused.tick, 21900);
assert.equal(draw(defused)[0].alpha, 0.5, 'Observed defusal styling changed');

const exploded = contextAt(22000, { ...baseMeta,
  bombEvents: [plant, { ...plant, kind: 'bomb_exploded', tick: 22000, sourceIndex: 2 }],
  observedBombSelection: { plantIndex: 0, explosionIndex: 1, defusalIndex: null } });
assert.equal(exploded.bombExploded.tick, 22000);
assert.equal(draw(exploded)[0].width, 60, 'Observed explosion rendering changed');

assert.equal(draw(contextAt(plantTick - 1)).length, 0, 'Rewinding entered unavailable carrier history');
const betweenRounds = contextAt(21000, { ...baseMeta, endTick: 21000 });
assert.equal(betweenRounds.bombPlanted.tick, plantTick, 'Inter-round event disappeared from the source context');
assert.equal(draw(betweenRounds).length, 0, 'Inter-round plant invented a playable bomb frame');
assert.equal(draw(contextAt(18000, { ...baseMeta, bombEvents: [], observedBombSelection: null })).length, 0);
assert.equal(draw(contextAt(22000, { ...baseMeta, observedBombSelection: null })).length, 0,
  'Ambiguous bomb selection became drawable');
console.log('PASS: native observed bomb drawing avoids unavailable carrier history and preserves plant, defusal, explosion, rewind, and inter-round events');
