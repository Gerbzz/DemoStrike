const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const assets = path.resolve(__dirname, '../ported/dist/assets');
const scripts = fs.readdirSync(assets)
  .filter((name) => name.endsWith('.js'))
  .map((name) => fs.readFileSync(path.join(assets, name), 'utf8'));
assert.ok(scripts.length > 0, 'No compiled renderer JavaScript exists');
const bundle = scripts.join('\n');
const styles = fs.readdirSync(assets)
  .filter((name) => name.endsWith('.css'))
  .map((name) => fs.readFileSync(path.join(assets, name), 'utf8'))
  .join('\n');
assert.ok(bundle.includes('csdm_request'), 'Tauri request adapter was not included');
assert.ok(!bundle.includes('REACT_STRICT_MODE_ENABLED'), 'Electron-only startup constant was left unresolved');
assert.ok(bundle.includes('Existing CS Demo Manager PostgreSQL data is untouched.'), 'Accurate local-library settings pane was not included');
assert.ok(bundle.includes('Could not open this view'), 'Ported recoverable error boundary was not included');
assert.ok(!bundle.includes('WS:: connecting to server'), 'Electron WebSocket client remained in the ported bundle');
assert.ok(styles.includes('.gap-x-8'), 'Original UI spacing classes were not generated');
console.log('PASS: ported original renderer contains the Tauri adapter and no Electron WebSocket startup');
