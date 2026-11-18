// Apply complete patch2 AFTER the exact ANSI/common patch1. No producer wiring.
import { readFileSync, existsSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
export const patchRoot=resolve(dirname(fileURLToPath(import.meta.url)),'..');
export const patchFile=join(patchRoot,'0002-terminal-profile.patch');
export const sharedUnicodeSha256='2167c46bf05f2ecbb35a247f11216d44664b52c47c63bee86947823ddfa1fc90';
export function runtimeDigest() {return createHash('sha256').update(readFileSync(patchFile)).digest('hex');}
export function apply(source) {
 const root=resolve(source);
 if(readFileSync(join(root,'LIB_VERSION'),'utf8').trim()!=='1.0.1') throw new Error('Not vt1.0.1');
 const unicode=readFileSync(join(root,'src/terminal/shellfox_unicode_v6.zig'));
 if(createHash('sha256').update(unicode).digest('hex')!==sharedUnicodeSha256) throw new Error('Apply the exact authoritative ANSI1 Unicode prerequisite first');
 if(existsSync(join(root,'src/terminal/shellfox_terminal_profile.zig'))) throw new Error('Profile module already exists; refusing to overwrite it');
 const args=['--batch','--fuzz=0','--no-backup-if-mismatch','-p1','-i',patchFile];
 for(const dry of [true,false]) {
  const result=spawnSync('patch',[...(dry?['--dry-run']:[]),...args],{cwd:root,encoding:'utf8',timeout:30000});
  if(result.error||result.status!==0) throw new Error(`Patch ${dry?'preflight':'apply'} failed: ${result.error??result.stderr??result.stdout}`);
 }
 return {checkpoint:'native-prerequisite-not-final',patchSha256:runtimeDigest(),legacyLiveEnabled:false};
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
 const source=process.argv[2];if(!source) throw new Error('usage: node tools/apply.mjs private-source-after-ANSI1');
 console.log(JSON.stringify(apply(source)));
}
