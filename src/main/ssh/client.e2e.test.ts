import { it, expect } from 'vitest';
import path from 'node:path';
import { mkdtemp, mkdir, copyFile, rm, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { build } from 'esbuild';
import { createRequire } from 'node:module';
import { helpText, sshHelp } from './presentation';
const exec=promisify(execFile);
it('connects through real shims in a PTY, forwards controls/resize, authenticates, checks TOFU and propagates exit',async()=>{
  await mkdir('tmp',{recursive:true});const directory=await mkdtemp(path.resolve('tmp/ssh-e2e-'));
  try {
    const helper=path.join(directory,'cli/main/cli.cjs'),driver=path.join(directory,'driver.cjs');
    const options={bundle:true,platform:'node' as const,format:'cjs' as const,target:'node24',external:['node-pty','cpu-features','*.node'],loader:{'.sh':'text' as const,'.ps1':'text' as const}};
    await build({...options,entryPoints:['src/main/ssh/cli.ts'],outfile:helper,define:{__SHELLFOX_VERSION__:JSON.stringify('test')}});
    await mkdir(path.join(directory,'cli/util'),{recursive:true});await copyFile('node_modules/ssh2/util/pagent.exe',path.join(directory,'cli/util/pagent.exe'));
    if(process.platform==='win32'){await mkdir(path.join(directory,'cli/cli-runtime'));await copyFile(process.execPath,path.join(directory,'cli/cli-runtime/node.exe'));}
    const help=await exec(process.execPath,[helper,'--help'],{timeout:5000});expect(help.stdout).toBe(helpText('test',false,process.platform!=='win32'));
    const sshUsage=await exec(process.execPath,[helper,'ssh','--help'],{timeout:5000});expect(sshUsage.stdout).toBe(sshHelp(false));
    await expect(exec(process.execPath,[helper,'ssh','--bad'],{timeout:5000})).rejects.toMatchObject({code:2,stdout:'  invalid ssh arguments\n  run shellfox --help\n'});
    await expect(exec(process.execPath,[helper,'--user-data',directory,'ssh'],{timeout:5000})).rejects.toMatchObject({code:2,stdout:expect.stringContaining('no ssh connections yet')});
    await build({...options,entryPoints:['tests/fixtures/ssh-e2e-driver.ts'],outfile:driver});
    // Linux node-pty builds against Electron's ABI. Run the native driver in
    // that same runtime, not the host Node used by Vitest. Windows uses N-API.
    const runtime=process.platform==='linux'?createRequire(import.meta.url)('electron') as string:process.execPath;
    const result=await exec(runtime,[driver,directory,helper],{timeout:120000,maxBuffer:1024*1024,env:{...process.env,...(process.platform==='linux'?{ELECTRON_RUN_AS_NODE:'1'}:{})}});
    await writeFile(path.resolve('tmp/ssh-e2e-evidence.json'),result.stdout);
    expect(JSON.parse(result.stdout)).toMatchObject({ok:true});
  }finally{await rm(directory,{recursive:true,force:true});}
},130000);
