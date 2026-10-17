import path from 'node:path';
import { fileURLToPath } from 'node:url';
import upstream from '../upstream/cs-demo-manager/vite.config.ts';

const portedRoot = path.dirname(fileURLToPath(import.meta.url));
const originalSource = path.resolve(portedRoot, '../upstream/cs-demo-manager/src');
const upstreamModules = path.resolve(portedRoot, '../upstream/cs-demo-manager/node_modules');

export default {
  ...upstream,
  root: portedRoot,
  resolve: {
    ...upstream.resolve,
    alias: [
      // The ported component lives outside the upstream package root, while
      // React Compiler adds this import after resolving its source file.
      { find: /^react$/, replacement: path.resolve(upstreamModules, 'react/index.js') },
      { find: 'react/compiler-runtime', replacement: path.resolve(upstreamModules, 'react/compiler-runtime.js') },
      { find: 'react/jsx-runtime', replacement: path.resolve(upstreamModules, 'react/jsx-runtime.js') },
      { find: 'csdm/ui/settings/database/database-settings', replacement: path.resolve(portedRoot, 'database-settings.tsx') },
      { find: 'csdm/ui/matches/table/matches-table', replacement: path.resolve(portedRoot, 'matches-table.tsx') },
      { find: 'csdm/ui/web-socket-client', replacement: path.resolve(portedRoot, 'web-socket-client.ts') },
      // The preserved provider imports this client relatively; redirect that
      // edge too so the app never attempts a localhost Electron WebSocket.
      { find: '../web-socket-client', replacement: path.resolve(portedRoot, 'web-socket-client.ts') },
      { find: /^csdm\//, replacement: `${originalSource}/` },
    ],
  },
  build: {
    ...upstream.build,
    outDir: path.resolve(portedRoot, 'dist'),
    emptyOutDir: true,
  },
};
