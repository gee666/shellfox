// Windows Electron is GUI-subsystem and cannot read a console TTY. Ship an
// unmodified, checksum-pinned console Node. WSL interop supplies a ConPTY too.
import { mkdir, readFile, writeFile, copyFile, rm, readdir } from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
const version = '24.15.0';
const hashes = {
  'win-x64/node.exe': '3331e1ffe19874215472217c5e94f5a0c6d8e18c4ac7111d3937aa0ad5e9b4a5',
  'win-arm64/node.exe': '49a54c103f4919ce64199a043ef5cd309507de491d718085edee089cd8e87543',
  LICENSE: '4573185d56580da2b890ba34a85a409257640f1c5632eade4300137266194d18',
};
export async function stageSshRuntime(output) {
  if (process.platform !== 'win32') {await rm(path.join(output,'cli-runtime'),{recursive:true,force:true});return;}
  const cache = path.resolve('tmp/ssh-runtime-cache'), target = path.join(output, 'cli-runtime');
  await mkdir(cache, { recursive: true }); await mkdir(target, { recursive: true });
  async function download(file) {
    const local = path.join(cache, file.replaceAll('/','-'));
    let data; try { data = await readFile(local); } catch {}
    if (!data || createHash('sha256').update(data).digest('hex') !== hashes[file]) {
      const response = await fetch(file==='LICENSE' ? `https://raw.githubusercontent.com/nodejs/node/v${version}/LICENSE` : `https://nodejs.org/dist/v${version}/${file}`, { signal: AbortSignal.timeout(120000) });
      if (!response.ok) throw new Error('Node CLI runtime download failed: ' + response.status);
      data = Buffer.from(await response.arrayBuffer());
      if (createHash('sha256').update(data).digest('hex') !== hashes[file]) throw new Error('Node CLI runtime checksum mismatch');
      await writeFile(local, data);
    }
    return local;
  }
  async function install(file,name){
    const source=await download(file),destination=path.join(target,name);
    try{if(createHash('sha256').update(await readFile(destination)).digest('hex')===hashes[file])return;}catch{}
    await copyFile(source,destination);
  }
  // An unchanged runtime may be executing a user's external SSH session.
  // Do not unlink/overwrite it just to rebuild the helper JavaScript.
  await install(`win-${process.arch}/node.exe`,'node.exe');
  await install('LICENSE','NODE-LICENSE');
  for(const name of await readdir(target))if(!['node.exe','NODE-LICENSE'].includes(name))await rm(path.join(target,name),{recursive:true,force:true});
}
if (process.argv[1]?.endsWith('ssh-runtime.mjs')) await stageSshRuntime(process.argv[2] ?? 'tmp/build');
