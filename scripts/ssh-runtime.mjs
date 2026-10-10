// Windows needs an unmodified console Node. No downloaded binary is executed
// by this build step. Keep network/bootstrap work out of Node's fetch/Undici
// path on Windows (native fail-fast cannot be caught by JavaScript).
import { mkdir, copyFile, rm, readdir, rename } from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const execute=promisify(execFile);
const version='24.15.0';
const hashes={
  'win-x64/node.exe':'3331e1ffe19874215472217c5e94f5a0c6d8e18c4ac7111d3937aa0ad5e9b4a5',
  'win-arm64/node.exe':'49a54c103f4919ce64199a043ef5cd309507de491d718085edee089cd8e87543',
  LICENSE:'4573185d56580da2b890ba34a85a409257640f1c5632eade4300137266194d18',
};
export async function fileSha256(file){
  const hash=createHash('sha256');
  for await(const chunk of createReadStream(file))hash.update(chunk);
  return hash.digest('hex');
}
async function matches(file,expected){
  try{return await fileSha256(file)===expected;}
  catch(error){if(error.code==='ENOENT')return false;throw error;}
}
export async function downloadRuntimeAsset(url,file){
  // Use Windows' system curl, not Git Bash's shim or a PATH dependency.
  const curl=path.win32.join(process.env.SystemRoot||'C:\\Windows','System32','curl.exe');
  try{
    await execute(curl,['--fail','--location','--proto','=https','--tlsv1.2','--connect-timeout','30','--max-time','120','--retry','2','--retry-delay','1','--silent','--show-error','--output',file,url],{timeout:370000,windowsHide:true,maxBuffer:65536});
  }catch(error){
    throw new Error(`curl download failed for ${url}: ${error.message}${error.stderr?.trim()?'\n'+error.stderr.trim():''}`,{cause:error});
  }
}
export async function stageSshRuntime(output,{download=downloadRuntimeAsset,log=console.log}={}){
  if(process.platform!=='win32'){
    log(`[ssh-runtime] ${process.platform}-${process.arch}: bundled Windows Node not required`);
    await rm(path.join(output,'cli-runtime'),{recursive:true,force:true});return;
  }
  const cache=path.resolve('tmp/ssh-runtime-cache'),target=path.resolve(output,'cli-runtime');
  log(`[ssh-runtime] staging Node ${version} win-${process.arch}; cache=${cache}; target=${target}`);
  await mkdir(cache,{recursive:true});await mkdir(target,{recursive:true});
  async function asset(file){
    const expected=hashes[file];if(!expected)throw new Error('Unsupported Node runtime asset: '+file);
    const local=path.join(cache,file.replaceAll('/','-'));
    if(await matches(local,expected)){log(`[ssh-runtime] verified cache ${file} SHA256:${expected}`);return local;}
    const url=file==='LICENSE'?`https://raw.githubusercontent.com/nodejs/node/v${version}/LICENSE`:`https://nodejs.org/dist/v${version}/${file}`;
    const partial=local+'.'+randomUUID()+'.part';
    log(`[ssh-runtime] downloading ${url}`);
    try{
      await download(url,partial);
      const actual=await fileSha256(partial);
      if(actual!==expected)throw new Error(`Node CLI runtime checksum mismatch for ${file}: expected ${expected}, received ${actual}`);
      await rename(partial,local);log(`[ssh-runtime] verified download ${file} SHA256:${actual}`);
      return local;
    }finally{await rm(partial,{force:true});}
  }
  async function install(file,name){
    const source=await asset(file),destination=path.join(target,name);
    if(await matches(destination,hashes[file])){log(`[ssh-runtime] unchanged ${name}`);return;}
    await copyFile(source,destination);
    if(!await matches(destination,hashes[file]))throw new Error('Staged Node runtime failed checksum verification: '+destination);
    log(`[ssh-runtime] installed ${destination}`);
  }
  try{
    await install(`win-${process.arch}/node.exe`,'node.exe');
    await install('LICENSE','NODE-LICENSE');
    for(const name of await readdir(target))if(!['node.exe','NODE-LICENSE'].includes(name))await rm(path.join(target,name),{recursive:true,force:true});
    log('[ssh-runtime] staging complete');
  }catch(error){throw new Error(`SSH runtime staging failed (win-${process.arch}, Node ${version}, cache ${cache}): ${error.message}`,{cause:error});}
}
if(process.argv[1]?.endsWith('ssh-runtime.mjs'))await stageSshRuntime(process.argv[2]??'tmp/build');
