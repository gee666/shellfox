import { build } from 'esbuild';
import path from 'node:path';
import { runNode, root } from './common.mjs';
const output = path.join(root, 'tmp/build-cache/pty-electron-test.cjs');
await build({
  entryPoints: ['src/main/terminal/pty.electron-test.ts'], outfile: output,
  bundle: true, platform: 'node', format: 'cjs', target: 'node24',
  external: ['electron', 'node-pty'], define: { __PROJECT_ROOT__: JSON.stringify(root) },
});
runNode('node_modules/electron/cli.js', [output], 60000);
