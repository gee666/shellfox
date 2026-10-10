import path from 'node:path';
import { homedir, userInfo } from 'node:os';
import { readdir, readFile } from 'node:fs/promises';
import { runRegistry, type RegistryRunner } from '../platform/explorer';
import { z } from 'zod';
import { normalizeSshEndpoint } from '../../shared/ssh-endpoint';
import { sshConnectionOptionsSchema, sshProfileInputSchema } from '../../shared/ssh-schemas';
export interface PuttySession { name: string; host: string; port: number; user: string; keyFile: string | null; connectionOptions?:z.infer<typeof sshConnectionOptionsSchema> }
export function parsePutty(name: string, values: Record<string, unknown>, localUsername?:string): PuttySession | null {
  if (typeof name!=='string' || !values || typeof values!=='object' || Array.isArray(values))return null;
  if (values.Protocol !== undefined && values.Protocol !== null && String(values.Protocol).trim().toLowerCase() !== 'ssh') return null;
  for(const field of ['HostName','UserName','PublicKeyFile'])if(values[field]!==undefined&&values[field]!==null&&typeof values[field]!=='string')return null;
  let decoded: string; try { decoded = decodeURIComponent(name); } catch { decoded = name; }
  const p=normalizeSshEndpoint({name:decoded,host:String(values.HostName??''),port:Number(values.PortNumber??22),user:String(values.UserName??''),keyFile:String(values.PublicKeyFile??'')||null});
  if(!p.user&&Number(values.UserNameFromEnvironment)===1)p.user=localUsername??userInfo().username;
  const connectionOptions:z.infer<typeof sshConnectionOptionsSchema>={};
  if(Number(values.AddressFamily)===1)connectionOptions.addressFamily=4;
  else if(Number(values.AddressFamily)===2)connectionOptions.addressFamily=6;
  if(Number(values.TryAgent)===0&&values.TryAgent!==undefined)connectionOptions.tryAgent=false;
  if(Number(values.AgentFwd)===1)connectionOptions.agentForward=true;
  let proxy=Number(values.ProxyMethod??-1);
  if(!Number.isInteger(proxy)||proxy<0||proxy>8){
    const old=Number(values.ProxyType??0);proxy=({0:0,1:3,2:Number(values.ProxySOCKSVersion)===4?1:2,3:4,4:5} as Record<number,number>)[old]??0;
  }
  if(proxy!==0){
    const method=['none','SOCKS4','SOCKS5','HTTP','Telnet','local command','SSH jump host','SSH command','SSH subsystem'][proxy];
    const rawPort=Number(values.ProxyPort??80);
    connectionOptions.unsupportedProxy={method,host:typeof values.ProxyHost==='string'?values.ProxyHost.replace(/[\x00-\x1f\x7f]/g,' ').slice(0,1000):null,port:Number.isInteger(rawPort)&&rawPort>=0&&rawPort<=65535?rawPort:null};
  }
  const valid=sshProfileInputSchema.safeParse({ ...p, id: '00000000-0000-4000-8000-000000000001', remoteCwd: null });
  if(!valid.success)return null;
  const {name:cleanName,host:cleanHost,user:cleanUser}=valid.data;
  const extras=sshConnectionOptionsSchema.safeParse(connectionOptions);
  return {...p,name:cleanName,host:cleanHost,user:cleanUser,...(extras.success&&Object.keys(extras.data).length?{connectionOptions:extras.data}:{})};
}
export function parsePuttyFile(name: string, text: string): PuttySession | null {
  const values: Record<string, string> = {};
  for (const line of text.split(/\r?\n/)) { const i = line.indexOf('='); if (i > 0) values[line.slice(0,i)] = line.slice(i+1); }
  return parsePutty(name, values);
}
export const PUTTY_REGISTRY_SCRIPT = `$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
try {
  $root=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\\SimonTatham\\PuTTY\\Sessions')
  $sessions=@()
  if ($null -ne $root) {
    try { foreach ($name in $root.GetSubKeyNames()) {
      $key=$null
      try {
        $key=$root.OpenSubKey($name)
        if ($null -eq $key) { continue }
        $values=@{}; $present=$key.GetValueNames()
        # Do not serialize the hundreds of unrelated PuTTY settings per session.
        foreach ($n in @('HostName','PortNumber','UserName','PublicKeyFile','Protocol','AddressFamily','ProxyMethod','ProxyType','ProxySOCKSVersion','ProxyHost','ProxyPort','AgentFwd','TryAgent','UserNameFromEnvironment')) {
          if ($present -contains $n) {
            $kind=$key.GetValueKind($n).ToString()
            if ($kind -eq 'String' -or $kind -eq 'DWord') { $values[$n]=$key.GetValue($n,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
          }
        }
        $sessions+=@{name=$name;values=$values}
      } catch { continue }
      finally { if ($null -ne $key) { $key.Dispose() } }
    } } finally { $root.Dispose() }
  }
  $localUsername=$null
  if (@($sessions | Where-Object { $_.values.UserNameFromEnvironment -eq 1 }).Count -gt 0) {
    # PuTTY get_username(): prefer the UPN's part before @, then local name.
    try {
      Add-Type -TypeDefinition 'using System; using System.Text; using System.Runtime.InteropServices; public class ShellfoxPuttyLocalUser { [DllImport("secur32.dll",CharSet=CharSet.Unicode,EntryPoint="GetUserNameExW")] [return:MarshalAs(UnmanagedType.U1)] static extern bool GetUserNameEx(int format,StringBuilder value,ref uint size); public static string Get() { uint size=0; GetUserNameEx(8,null,ref size); var value=new StringBuilder((int)Math.Max(size,256)); size=(uint)value.Capacity; if(GetUserNameEx(8,value,ref size))return value.ToString().Split(''@'')[0]; return Environment.UserName; } }'
      $localUsername=[ShellfoxPuttyLocalUser]::Get()
    } catch { $localUsername=[Environment]::UserName }
  }
  @{ok=$true;value=@($sessions);localUsername=$localUsername} | ConvertTo-Json -Depth 8 -Compress
} catch { @{ok=$false;error=@{code='STORAGE_FAILED';message='Could not read PuTTY registry sessions.';retryable=$true}} | ConvertTo-Json -Compress }
`;
export async function readPuttySessions(options: { platform?: NodeJS.Platform; home?: string; run?: RegistryRunner } = {}): Promise<PuttySession[]> {
  if ((options.platform ?? process.platform) === 'win32') {
    let result: { ok?:boolean; error?:{message?:string}; value?:unknown[]; localUsername?:string };
    try { result=await (options.run ?? (script=>runRegistry(script,{maxBuffer:8*1024*1024})))(PUTTY_REGISTRY_SCRIPT) as typeof result; }
    catch(e){
      const error=e as NodeJS.ErrnoException & {killed?:boolean};
      if(error.code==='ERR_CHILD_PROCESS_STDIO_MAXBUFFER')throw new Error('PuTTY sessions exceed the 8 MiB import limit.');
      if(error.killed)throw new Error('PuTTY registry import timed out. Try again.');
      throw new Error('Could not read PuTTY registry sessions: '+(error.message || 'PowerShell failed.'));
    }
    if (!result || !result.ok || !Array.isArray(result.value)) throw new Error(result?.error?.message || 'PuTTY registry returned invalid session data.');
    const parsed:PuttySession[]=[];
    for(const row of result.value){
      if(!row||typeof row!=='object')continue;
      const s=row as {name:string;values:Record<string,unknown>};
      const session=parsePutty(s.name,s.values,typeof result.localUsername==='string'?result.localUsername:undefined);if(session)parsed.push(session);
    }
    return parsed;
  }
  const directory = path.join(options.home ?? homedir(), '.putty', 'sessions');
  let names: string[];
  try { names = await readdir(directory); } catch (e) { if ((e as NodeJS.ErrnoException).code === 'ENOENT') return []; throw e; }
  const sessions: PuttySession[] = [];
  for (const name of names) {
    try { const p=parsePuttyFile(name,await readFile(path.join(directory,name),'utf8'));if(p)sessions.push(p); }
    catch(e){if(!['EISDIR','ENOENT','EACCES'].includes((e as NodeJS.ErrnoException).code??''))throw new Error('Could not read a PuTTY session file: '+(e as Error).message);}
  }
  return sessions;
}
