import { expect, it } from 'vitest';
import { parsePutty, readPuttySessions, PUTTY_REGISTRY_SCRIPT } from './putty';
it('projects only import fields from the large real-world PuTTY shape, avoiding the old 64 KiB failure',async()=>{
 // Sanitised reproduction: 15 sessions, 239-286 values per key. The real
 // registry payload was 85,730 bytes; no real names/hosts appear in this test.
 const all=Array.from({length:15},(_,i)=>({name:i===0?'Default%20Settings':'connection%20'+i,values:{HostName:i===0?'':'fixture'+i+'.example.invalid',PortNumber:22,UserName:'',Protocol:'ssh',PublicKeyFile:'C:\\Keys with spaces\\ключ.ppk',...Object.fromEntries(Array.from({length:250},(_,j)=>['UnrelatedSetting'+j,'x'.repeat(16)]))}}));
 expect(Buffer.byteLength(JSON.stringify(all))).toBeGreaterThan(65536);
 const fields=['HostName','PortNumber','UserName','PublicKeyFile','Protocol'];
 const rows=await readPuttySessions({platform:'win32',run:async script=>{
  expect(script).toContain("@('HostName','PortNumber','UserName','PublicKeyFile','Protocol',");
  expect(script).not.toContain('foreach ($n in $key.GetValueNames())');
  expect(script).toContain('OutputEncoding');
  return {ok:true,value:all.map(s=>({...s,values:Object.fromEntries(fields.map(f=>[f,(s.values as Record<string,unknown>)[f]]))}))};
 }});
 expect(rows).toHaveLength(14);expect(rows[0].keyFile).toContain('ключ');expect(PUTTY_REGISTRY_SCRIPT).not.toContain('SetValue');
});
it('skips malformed individual rows and returns canonical trimmed names',async()=>{
 const sessions=await readPuttySessions({platform:'win32',run:async()=>({ok:true,value:[null,{}, {name:'bad',values:null},{name:'space%20',values:{HostName:'user@host.example.invalid',PortNumber:22}}, {name:'SPACE',values:{HostName:'host.example.invalid'}},{name:'zero',values:{HostName:'host',PortNumber:0}},{name:'long'.repeat(100),values:{HostName:'host'}},{name:'%00bad',values:{HostName:'host'}},{name:'telnet',values:{HostName:'host',Protocol:'telnet'}}]})});
 expect(sessions).toHaveLength(2);expect(sessions[0]).toMatchObject({name:'space',host:'host.example.invalid',user:'user'});
 expect(parsePutty('ok',{HostName:'host',Protocol:null})).toMatchObject({port:22,user:''});
});
it('mirrors PuTTY user precedence, address family, agent and proxy metadata without expanding key filenames',()=>{
 expect(parsePutty('sample',{HostName:'login@host.test:55',UserName:'ignored',PortNumber:2222,AddressFamily:2,AgentFwd:1,TryAgent:0,PublicKeyFile:'%HOME%\\\\key.ppk'})).toMatchObject({host:'host.test',user:'login',port:2222,keyFile:'%HOME%\\\\key.ppk',connectionOptions:{addressFamily:6,agentForward:true,tryAgent:false}});
 expect(parsePutty('local',{HostName:'host.test',UserNameFromEnvironment:1},'local-user')).toMatchObject({user:'local-user'});
 expect(parsePutty('local',{HostName:'preferred@host.test',UserNameFromEnvironment:1},'local-user')).toMatchObject({user:'preferred'});
 expect(parsePutty('proxy',{HostName:'host.test',ProxyMethod:6,ProxyHost:'jump.test',ProxyPort:22})).toMatchObject({connectionOptions:{unsupportedProxy:{method:'SSH jump host',host:'jump.test',port:22}}});
 expect(parsePutty('proxy',{HostName:'host.test',ProxyType:2,ProxySOCKSVersion:4})).toMatchObject({connectionOptions:{unsupportedProxy:{method:'SOCKS4'}}});
});
it('reports specific runner and data failures',async()=>{
 await expect(readPuttySessions({platform:'win32',run:async()=>{throw Object.assign(new Error('maxBuffer'),{code:'ERR_CHILD_PROCESS_STDIO_MAXBUFFER'});}})).rejects.toThrow('8 MiB import limit');
 await expect(readPuttySessions({platform:'win32',run:async()=>({ok:false,error:{message:'Access denied reading PuTTY registry.'}})})).rejects.toThrow('Access denied');
 await expect(readPuttySessions({platform:'win32',run:async()=>({ok:true,value:'invalid'})})).rejects.toThrow('invalid session data');
});
