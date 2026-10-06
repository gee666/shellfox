// Isolated Linux dependency/build tree. Never reuse Windows node_modules for a .deb.
import { cpSync, mkdirSync, rmSync } from 'node:fs';
import path from 'node:path';
const stage=path.resolve('tmp/ubuntu-source');
rmSync(stage,{recursive:true,force:true});mkdirSync(stage,{recursive:true});
for(const item of ['package.json','pnpm-lock.yaml','pnpm-workspace.yaml','.npmrc','forge.config.cjs','tsconfig.json','vite.config.ts','vitest.config.ts','scripts','src','resources','tests','native'])cpSync(item,path.join(stage,item),{recursive:true});
// Embedded runtime builds node-pty on Linux; no prepublished .NET helper needed.
mkdirSync(path.join(stage,'tmp'),{recursive:true});
console.log(stage);
