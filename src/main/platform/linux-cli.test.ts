import { expect, it } from 'vitest';
import path from 'node:path';
import { mkdir, mkdtemp, writeFile, chmod, symlink, readFile, rm } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { linuxLauncher, LinuxCliIntegration, ownsLinuxFile, shQuote } from './linux-cli';
import { build } from 'esbuild';
const exec=promisify(execFile);
it.skipIf(process.platform!=='linux')('real sh launcher handles cold spawn, spaces/quotes, symlinks, leading dashes and missing paths',async()=>{
 await mkdir(path.resolve('tmp'),{recursive:true});const root=await mkdtemp(path.resolve('tmp/linux-launcher-'));
 try{
  const output=path.join(root,'argv.txt'),fake=path.join(root,'fake'),shim=path.join(root,'shellfox'),folder=path.join(root,"space 'quote' & %");await mkdir(folder);await mkdir(path.join(root,'--leading'));await mkdir(path.join(root,'-'));await symlink(folder,path.join(root,'link'));const physical=path.join(root,'physical','child');await mkdir(physical,{recursive:true});await symlink(physical,path.join(root,'deep-link'));
  const helper = path.join(root, 'cli.cjs');
  await build({ entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: helper, bundle: true, platform: 'node', format: 'cjs', packages: 'external', loader: { '.sh': 'text' }, define: { __SHELLFOX_VERSION__: '"test"' } });
  const runtime = `#!/bin/sh\nif [ "\${ELECTRON_RUN_AS_NODE:-}" = 1 ]; then exec ${shQuote(process.execPath)} "$@"; fi\n`;
  await writeFile(fake,runtime+'printf "%s\\n" "$@" > "$SHELLFOX_LAUNCH_TEST"\n');await chmod(fake,0o755);await writeFile(shim,linuxLauncher({executable:fake,cliHelper:helper,userData:path.join(root,'data')}));await chmod(shim,0o755);
  for(const [alias, target] of ['.',"space 'quote' & %",'link','--leading','-','deep-link/..'].map(target=>['session',target]).concat(['ss','sessions'].map(alias=>[alias,'.']))){
   const result=await exec('/bin/sh',[shim,alias,'create',...(target.startsWith('--')?['--']:[]),target],{cwd:root,env:{...process.env,SHELLFOX_LAUNCH_TEST:output,OLDPWD:folder},timeout:3000});expect(result.stdout).toContain('Shellfox: started session');
   const args=await readFile(output,'utf8');expect(args.split('\n')[0]).toBe('start');expect(args).toContain(target==='.'?root:target==='link'?folder:target==='deep-link/..'?path.dirname(physical):path.join(root,target));
  }
  await expect(exec('/bin/sh',[shim,'session','create','--leading'],{cwd:root,timeout:3000})).rejects.toMatchObject({code:2,stderr:expect.stringContaining('Unknown option: --leading')});
  await writeFile(fake,runtime+'sleep 2\n');
  const before=Date.now();
  await exec('/bin/sh',[shim,'session','create','.'],{cwd:root,timeout:4000});
  expect(Date.now()-before).toBeLessThan(1500);
  await new Promise(resolve=>setTimeout(resolve,2200)); // Let only our fake child exit naturally.
  await expect(exec('/bin/sh',[shim,'session','create','missing'],{cwd:root,timeout:3000})).rejects.toMatchObject({code:1});expect(await readFile(shim,'utf8')).toContain(helper);
 }finally{await rm(root,{recursive:true,force:true});}
},30000);
it.skipIf(process.platform!=='linux')('routes control commands, help and version to the helper with argv, data and exit code intact', async()=>{
 await mkdir(path.resolve('tmp'),{recursive:true}); const root=await mkdtemp(path.resolve('tmp/linux-cli-routing-'));
 try {
  const fake=path.join(root,'runtime'), shim=path.join(root,'shellfox'), helper=path.join(root,'cli helper'), data=path.join(root,"data 'quoted'");
  await writeFile(fake,'#!/bin/sh\nprintf "%s\\n" "$@"\nexit 17\n'); await chmod(fake,0o755);
  await writeFile(shim,linuxLauncher({executable:fake,cliHelper:helper,userData:data}));
  for(const args of [['session','list','--name','two words'],['ss','stats','--current'],['session','processes','--pid','42'],['session','delete'],['session','settle'],['session','unsettle'],['tm','create'],['host','keep-awake'],['--version'],['-v'],['version'],['-h'],['help','session'],['session','create','--help'],['ss','-h'],['terminals','help'],['unknown']]) {
   const result=await exec('/bin/sh',[shim,...args],{cwd:root,timeout:3000}).catch(error=>error);
   expect(result.code).toBe(17); expect(result.stdout).toBe([helper,'--shellfox-default-user-data',data,...args,''].join('\n'));
  }
 } finally { await rm(root,{recursive:true,force:true}); }
});
it('requires an exact ownership comment and preserves a foreign launcher',async()=>{
 expect(ownsLinuxFile('# Documentation: Shellfox/linux-v1')).toBe(false);
 expect(ownsLinuxFile('echo Shellfox/linux-v1')).toBe(false);
 expect(ownsLinuxFile('# Shellfox/linux-v1\n')).toBe(true);
 await mkdir(path.resolve('tmp'),{recursive:true});const home=await mkdtemp(path.resolve('tmp/linux-cli-owner-'));
 try{
  const cli=new LinuxCliIntegration({executable:'/fake/app',home});await mkdir(cli.binDir,{recursive:true});
  const content='#!/bin/sh\n# Shellfox/linux-v1 is mentioned, not owned\n';await writeFile(cli.launcher,content);
  expect(await cli.set(true)).toMatchObject({ok:false,error:{code:'AUTH_FAILED'}});
  expect(await cli.set(false)).toMatchObject({ok:false,error:{code:'AUTH_FAILED'}});
  expect(await readFile(cli.launcher,'utf8')).toBe(content);
 }finally{await rm(home,{recursive:true,force:true});}
});
it.skipIf(process.platform!=='linux')('does not overwrite foreign symbolic-link targets even with an owner comment',async()=>{
 const home=await mkdtemp(path.resolve('tmp/linux-cli-link-'));
 try{
  const cli=new LinuxCliIntegration({executable:'/fake/app',home});await mkdir(cli.binDir,{recursive:true});
  const target=path.join(home,'foreign-target'),content='# Shellfox/linux-v1\nforeign target\n';await writeFile(target,content);await symlink(target,cli.launcher);
  expect(await cli.set(true)).toMatchObject({ok:false,error:{code:'AUTH_FAILED'}});
  expect(await readFile(target,'utf8')).toBe(content);
 }finally{await rm(home,{recursive:true,force:true});}
});
it('never adds a Chromium sandbox bypass or interpolates folder data into source',()=>{const content=linuxLauncher({executable:'/opt/Shellfox app/shellfox'});expect(content).not.toContain('--no-sandbox');expect(content).toContain('SHELLFOX_CLI_LAUNCH=');expect(content).toContain('"$@"');expect(content).toContain('exec');expect(content).not.toContain('launch &');});
