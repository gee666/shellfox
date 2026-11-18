import { it, expect } from 'vitest';
import path from 'node:path';
import { mkdtemp, mkdir, copyFile, rm, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { build, type Plugin } from 'esbuild';
import { createRequire } from 'node:module';
import { helpText, sshHelp } from './presentation';
import { sshTestEnvironment } from '../../../tests/fixtures/ssh-test-environment';
const exec=promisify(execFile);
const denyAgent: Plugin = { name: 'fixture-deny-agent', setup(builder) {
  builder.onResolve({ filter: /^\.\/agents$/ }, args => {
    if (path.resolve(args.importer) === path.resolve('src/main/ssh/client.ts')) return { path: 'agents', namespace: 'fixture-agent' };
  });
  builder.onLoad({ filter: /^agents$/, namespace: 'fixture-agent' }, () => ({ contents: `
    function deny() { throw new Error('Unexpected SSH agent invocation in fixture'); }
    export { deny as agentEndpoints, deny as availableAgent };
  ` }));
} };
it('connects through real shims in a PTY, forwards controls/resize, authenticates, checks TOFU and propagates exit',async()=>{
  if (process.env.SHELLFOX_SSH_TEST_WSL === '1' || process.env.SHELLFOX_SSH_TEST_LINUX_ELECTRON) {
    throw new Error('WSL_REMOTE_FIXTURE_HOLD: loopback-only SSH fixture has no approved remote-route owner');
  }
  await mkdir('tmp',{recursive:true});const directory=await mkdtemp(path.resolve('tmp/ssh-e2e-'));
  const env=sshTestEnvironment(directory);
  try {
    const helper=path.join(directory,'cli/main/cli.cjs'),driver=path.join(directory,'driver.cjs');
    const options={bundle:true,platform:'node' as const,format:'cjs' as const,target:'node24',external:['node-pty','cpu-features','*.node'],loader:{'.sh':'text' as const,'.ps1':'text' as const}};
    await build({...options,entryPoints:['src/main/platform/shellfox-cli-entry.ts'],outfile:helper,plugins:[denyAgent],define:{__SHELLFOX_VERSION__:JSON.stringify('test')}});
    await mkdir(path.join(directory,'cli/util'),{recursive:true});await copyFile('node_modules/ssh2/util/pagent.exe',path.join(directory,'cli/util/pagent.exe'));
    if(process.platform==='win32'){await mkdir(path.join(directory,'cli/cli-runtime'));await copyFile(process.execPath,path.join(directory,'cli/cli-runtime/node.exe'));}
    const help=await exec(process.execPath,[helper,'--help'],{timeout:5000,env});expect(help.stdout).toBe(helpText('test',false,process.platform!=='win32'));
    const sshUsage=await exec(process.execPath,[helper,'ssh','--help'],{timeout:5000,env});expect(sshUsage.stdout).toBe(sshHelp(false));
    await expect(exec(process.execPath,[helper,'ssh','--bad'],{timeout:5000,env})).rejects.toMatchObject({code:2,stdout:'  invalid ssh arguments\n  run shellfox --help\n'});
    await expect(exec(process.execPath,[helper,'--user-data',directory,'ssh'],{timeout:5000,env})).rejects.toMatchObject({code:2,stdout:expect.stringContaining('no ssh connections yet')});
    await build({...options,entryPoints:['tests/fixtures/ssh-e2e-driver.ts'],outfile:driver});
    // Linux node-pty builds against Electron's ABI. Run the native driver in
    // that same runtime, not the host Node used by Vitest. Windows uses N-API.
    const runtime=process.platform==='linux'?createRequire(import.meta.url)('electron') as string:process.execPath;
    const result=await exec(runtime,[driver,directory,helper],{timeout:120000,maxBuffer:1024*1024,env:{...env,...(process.platform==='linux'?{ELECTRON_RUN_AS_NODE:'1'}:{})}});
    await writeFile(path.resolve('tmp/ssh-e2e-evidence.json'),result.stdout);
    expect(JSON.parse(result.stdout)).toMatchObject({ok:true});
  }finally{await rm(directory,{recursive:true,force:true});}
},130000);
