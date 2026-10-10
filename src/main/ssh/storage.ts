import path from 'node:path';
import { mkdir, readFile, writeFile, rename, unlink, chmod } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import { sshFileSchema, sshProfileInputSchema, sshStoredProfileSchema } from '../../shared/ssh-schemas';
import { failure, success, type Result, type SshProfileInput, type SshProfileDto, type SshImportResultDto } from '../../shared/contracts';
import type { PuttySession } from './putty';
export type StoredProfile = z.infer<typeof sshStoredProfileSchema>;
export const publicProfiles = (profiles: StoredProfile[]): SshProfileDto[] => profiles.map(({ password, connectionOptions: _privateOptions, ...p }) => ({ ...p, hasPassword: password !== null })).sort((a,b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
export async function atomicJson(file: string, value: unknown): Promise<void> {
  await mkdir(path.dirname(file), { recursive: true });
  const stage = file + '.' + randomUUID() + '.tmp';
  try {
    await writeFile(stage, JSON.stringify(value, null, 2) + '\n', { mode: 0o600, flag: 'wx' });
    await rename(stage, file);
    if (process.platform !== 'win32') await chmod(file, 0o600);
  } finally { await unlink(stage).catch(e => { if (e.code !== 'ENOENT') throw e; }); }
}
export class SshProfileStore {
  readonly file: string;
  private queue: Promise<unknown> = Promise.resolve();
  private normalizationIds=new Set<string>();
  constructor(readonly userData: string) { this.file = path.join(userData, 'ssh-profiles.json'); }
  async read(): Promise<StoredProfile[]> {
    let content: string;
    try { content = await readFile(this.file, 'utf8'); } catch (e) { if ((e as NodeJS.ErrnoException).code === 'ENOENT') return []; throw e; }
    try {
      const raw=JSON.parse(content),profiles=sshFileSchema.parse(raw).profiles;
      this.normalizationIds=new Set(profiles.filter((p,i)=>['name','host','user','port'].some(key=>raw.profiles[i]?.[key]!==p[key as keyof StoredProfile])).map(p=>p.id));
      return profiles;
    }
    catch { throw new Error('ssh-profiles.json is corrupt; repair it before saving connections.'); }
  }
  async list(): Promise<Result<SshProfileDto[]>> {
    try { await this.queue; return success(publicProfiles(await this.read())); }
    catch (e) { return failure('STORAGE_FAILED', (e as Error).message); }
  }
  private mutate<T>(action: (profiles: StoredProfile[]) => Promise<Result<T>>): Promise<Result<T>> {
    const operation = this.queue.then(async () => action(await this.read())).catch((e: Error) => failure('STORAGE_FAILED', e.message)) as Promise<Result<T>>;
    this.queue = operation.then(() => undefined); return operation;
  }
  private async write(profiles: StoredProfile[]) { await atomicJson(this.file, sshFileSchema.parse({ version: 1, profiles })); }
  save(input: SshProfileInput): Promise<Result<SshProfileDto[]>> {
    const valid = sshProfileInputSchema.safeParse(input);
    if (!valid.success) return Promise.resolve(failure('VALIDATION', 'Name, host and a port from 1 to 65535 are required.'));
    return this.mutate(async profiles => {
      const p = valid.data, previous = profiles.find(v => v.id === p.id);
      if (profiles.some(v => v.id !== p.id && v.name.toLowerCase() === p.name.toLowerCase())) return failure('VALIDATION', 'A connection with this name already exists.');
      const saved: StoredProfile = { ...p, password: p.password === undefined ? previous?.password ?? null : p.password, source: previous?.source ?? 'manual', ...(previous?.connectionOptions ? {connectionOptions:previous.connectionOptions}: {}) };
      const next = profiles.filter(v => v.id !== p.id).concat(saved); await this.write(next); return success(publicProfiles(next));
    });
  }
  delete(id: string): Promise<Result<SshProfileDto[]>> {
    return this.mutate(async profiles => { const next = profiles.filter(p => p.id !== id); await this.write(next); return success(publicProfiles(next)); });
  }
  import(sessions: PuttySession[]): Promise<Result<SshImportResultDto>> {
    return this.mutate(async profiles => {
      let added = 0, updated = 0, found=0;
      for (const raw of sessions) {
        const valid=sshProfileInputSchema.safeParse({id:'00000000-0000-4000-8000-000000000001',name:raw?.name,host:raw?.host,port:raw?.port,user:raw?.user,keyFile:raw?.keyFile,remoteCwd:null});
        if(!valid.success)continue;
        found++;
        const {name,host,port,user,keyFile}=valid.data,s={name,host,port,user,keyFile,connectionOptions:raw.connectionOptions};
        const previous = profiles.find(p => p.name.toLowerCase() === s.name.toLowerCase());
        if (previous) {
          if (this.normalizationIds.has(previous.id) || previous.host !== s.host || previous.port !== s.port || previous.user !== s.user || previous.keyFile !== s.keyFile || JSON.stringify(previous.connectionOptions??{})!==JSON.stringify(s.connectionOptions??{})) { Object.assign(previous, { host: s.host, port: s.port, user: s.user, keyFile: s.keyFile,connectionOptions:s.connectionOptions }); updated++; }
        } else {
          if(profiles.length>=10000)return failure('VALIDATION','At most 10,000 SSH connections can be saved. Import fewer sessions.');
          profiles.push({ ...s, id: randomUUID(), password: null, remoteCwd: null, source: 'putty' }); added++;
        }
      }
      if (added || updated) await this.write(profiles);
      return success({ profiles: publicProfiles(profiles), added, updated, found });
    });
  }
}
