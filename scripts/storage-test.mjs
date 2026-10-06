import { build } from 'esbuild';
import path from 'node:path';
import { runNode, root } from './common.mjs';
const output = 'tmp/build-cache/repository-electron-test.cjs';
await build({bundle:true,platform:'node',format:'cjs',target:'node24',external:['electron','better-sqlite3'],entryPoints:['src/main/repository.electron-test.ts'],outfile:output,define:{__PROJECT_ROOT__:JSON.stringify(root)}});
runNode('node_modules/electron/cli.js', [path.resolve(output)], 60000);
