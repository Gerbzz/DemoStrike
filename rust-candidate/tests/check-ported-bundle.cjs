const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const assets = path.resolve(__dirname, '../ported/dist/assets');
const scripts = fs.readdirSync(assets)
  .filter((name) => name.endsWith('.js'))
  .map((name) => fs.readFileSync(path.join(assets, name), 'utf8'));
assert.ok(scripts.length > 0, 'No compiled renderer JavaScript exists');
const bundle = scripts.join('\n');
assert.ok(bundle.includes('csdm_request'), 'Tauri request adapter was not included');
assert.ok(!bundle.includes('WS:: connecting to server'), 'Electron WebSocket client remained in the ported bundle');
console.log('PASS: ported original renderer contains the Tauri adapter and no Electron WebSocket startup');
