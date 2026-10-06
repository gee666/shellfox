import { runNode } from './common.mjs';
runNode('scripts/build.mjs', [], 600000);
runNode('node_modules/electron/cli.js', ['.', ...process.argv.slice(2)], 86400000);
