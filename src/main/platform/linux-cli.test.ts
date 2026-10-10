import { expect, it } from 'vitest';
import path from 'node:path';
import { mkdir, mkdtemp, writeFile, chmod, symlink, readFile, rm } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { linuxLauncher, LinuxCliIntegration, ownsLinuxFile } from './linux-cli';
const exec=promisify(execFile);
it.skipIf(process.platform!=='linux')('real sh launcher handles cold spawn, spaces/quotes, symlinks, leading dashes and missing paths',async()=>{
 await mkdir(path.resolve('tmp'),{recursive:true});const root=await mkdtemp(path.resolve('tmp/linux-launcher-'));
 try{
  const output=path.join(root,'argv.txt'),fake=path.join(root,'fake'),shim=path.join(root,'shellfox'),folder=path.join(root,"space 'quote' & %");await mkdir(folder);await mkdir(path.join(root,'--leading'));await mkdir(path.join(root,'-'));await symlink(folder,path.join(root,'link'));const physical=path.join(root,'physical','child');await mkdir(physical,{recursive:true});await symlink(physical,path.join(root,'deep-link'));
  await writeFile(fake,'#!/bin/sh\nprintf "%s\\n" "$@" > "$SHELLFOX_LAUNCH_TEST"\n');await chmod(fake,0o755);await writeFile(shim,linuxLauncher({executable:fake}));await chmod(shim,0o755);
  for(const target of ['.',"space 'quote' & %",'link','--leading','-','deep-link/..']){
   const result=await exec('/bin/sh',[shim,'start',target],{cwd:root,env:{...process.env,SHELLFOX_LAUNCH_TEST:output,OLDPWD:folder},timeout:3000});expect(result.stdout).toContain('Shellfox: started session');
   const args=await readFile(output,'utf8');expect(args.split('\n')[0]).toBe('start');expect(args).toContain(target==='.'?root:target==='link'?folder:target==='deep-link/..'?path.dirname(physical):path.join(root,target));
  }
  await writeFile(fake,'#!/bin/sh\nsleep 2\n');
  const before=Date.now();
  await exec('/bin/sh',[shim],{cwd:root,timeout:4000});
  expect(Date.now()-before).toBeLessThan(1500);
  await new Promise(resolve=>setTimeout(resolve,2200)); // Let only our fake child exit naturally.
  await expect(exec('/bin/sh',[shim,'start','missing'],{cwd:root,timeout:3000})).rejects.toMatchObject({code:1});expect(await readFile(shim,'utf8')).toContain('cli/main/cli.cjs');
 }finally{await rm(root,{recursive:true,force:true});}
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
it('never adds a Chromium sandbox bypass or interpolates folder data into source',()=>{const content=linuxLauncher({executable:'/opt/Shellfox app/shellfox'});expect(content).not.toContain('--no-sandbox');expect(content).toContain('launch start "$folder"');expect(content).toContain('setsid --fork --wait');});
