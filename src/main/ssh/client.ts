import { Client, type AnyAuthMethod, type ClientChannel } from 'ssh2';
import { createConnection, type Socket } from 'node:net';
import { readFile, mkdir } from 'node:fs/promises';
import { constants } from 'node:os';
import type { StoredProfile } from './storage';
import { CliTerminal, Cancelled } from './terminal';
import { remoteCommand, styles, targetLabel } from './presentation';
import { trustHost } from './known-hosts';
import { parsePrivateKey } from './keys';
import { remotePty, remoteEnvironment } from './pty';
import { normalizeSshEndpoint } from '../../shared/ssh-endpoint';
import { agentEndpoints, availableAgent } from './agents';
export class SshConnectionError extends Error {}
export class ConnectionLost extends SshConnectionError {}
export interface SshConnectOptions {
  /** Injectable only for backend tests, not CLI/renderer settings. */
  keepaliveIntervalMs?:number;
  stdin?:NodeJS.ReadStream;
  stdout?:NodeJS.WriteStream;
  onReady?:()=>void;
  onSocket?:(socket:Socket)=>void;
}
export async function connectSsh(profile: StoredProfile, userData: string, terminal: CliTerminal, options:SshConnectOptions = {}): Promise<number> {
  profile=normalizeSshEndpoint(profile);
  if(!profile.host||/@|\s/u.test(profile.host))throw new Error('invalid SSH host; enter a hostname or IP address without @ or whitespace');
  const proxy=profile.connectionOptions?.unsupportedProxy;
  if(proxy)throw new SshConnectionError('PuTTY profile requires '+proxy.method+' proxy/jump host'+(proxy.host?' '+proxy.host+(proxy.port?':'+proxy.port:''):'')+'; proxies and jump hosts are not supported. Disable the proxy in PuTTY and re-import, or add a direct manual connection.');
  const stdin=options.stdin??process.stdin,stdout=options.stdout??process.stdout;
  const s=styles(terminal.color),username=profile.user||await terminal.prompt('login as: ');
  if(!username)throw new Error('login name is required');
  const prompts=new AbortController(),prompt=(label:string,hidden=false)=>terminal.prompt(label,hidden,prompts.signal);
  const methods:AnyAuthMethod[]=[];
  if(profile.keyFile){
    const data=await readFile(profile.keyFile);let parsed;
    try{parsed=await parsePrivateKey(data);}catch(e){
      if(!/encrypted|passphrase/i.test((e as Error).message))throw e;
      for(let i=0;i<3;i++){
        try{parsed=await parsePrivateKey(data,await terminal.prompt('Passphrase: ',true));break;}
        catch(error){if(error instanceof Cancelled||i===2)throw error;stdout.write('  '+s.dim('incorrect passphrase')+'\n');}
      }
    }
    if(!parsed)throw new Error('could not read private key');
    methods.push({type:'publickey',username,key:parsed});
  }
  if(profile.connectionOptions?.tryAgent!==false)for(const agent of agentEndpoints())methods.push({type:'agent',username,agent});
  const forward=profile.connectionOptions?.agentForward?await availableAgent():null;
  if(profile.connectionOptions?.agentForward&&!forward)stdout.write('  '+s.dim('no SSH agent available; agent forwarding is unavailable')+'\n');
  if(profile.password!==null)methods.push({type:'password',username,password:profile.password});
  let authError:Error|undefined;
  const connection=new Client();
  methods.push({type:'keyboard-interactive',username,prompt:(_name,instructions,_lang,questions,answer)=>{
    void(async()=>{
      if(instructions)stdout.write('  '+s.dim(instructions.replace(/[\x00-\x1f\x7f]/g,' '))+'\n');
      const replies:string[]=[];for(const p of questions)replies.push(await prompt(p.prompt.replace(/[\x00-\x1f\x7f]/g,' '),!p.echo));answer(replies);
    })().catch(e=>{authError=e;connection.destroy();});
  }});
  await mkdir(userData,{recursive:true});
  stdout.write('  '+s.dim('connecting to '+targetLabel({...profile,user:username})+' ...')+'\n');
  return new Promise<number>((resolve,reject)=>{
    let index=0,passwordTries=profile.password===null?0:1,channel:ClientChannel|undefined,exitCode=0,finished=false,ready=false,remoteExited=false;
    const resize=()=>channel?.setWindow(stdout.rows||24,stdout.columns||80,0,0);
    const endInput=()=>channel?.end();
    const input=(bytes:Buffer)=>{if(channel&&!channel.write(bytes))stdin.pause();};
    const output=(bytes:Buffer)=>{if(!stdout.write(bytes)){channel?.pause();channel?.stderr.pause();}};
    const drain=()=>{channel?.resume();channel?.stderr.resume();};
    const cancel=()=>{authError=new Cancelled();connection.destroy();};
    const finish=(error?:Error)=>{
      if(finished)return;finished=true;prompts.abort();
      stdin.off('data',input);stdin.off('end',endInput);stdin.off('error',localError);
      stdout.off('resize',resize);stdout.off('drain',drain);stdout.off('error',localError);
      terminal.abort.signal.removeEventListener('abort',cancel);
      if(error)connection.destroy();else connection.end();
      if(error)reject(error);else resolve(exitCode);
    };
    const localError=(error:Error)=>finish(error);
    terminal.abort.signal.addEventListener('abort',cancel,{once:true});
    if(terminal.abort.signal.aborted){cancel();finish(authError);return;}
    connection.on('error',e=>{
      const level=(e as Error&{level?:string}).level;
      if(!ready&&level==='agent')return;
      if(authError){finish(authError);return;}if(remoteExited){finish();return;}
      if(ready){finish(new ConnectionLost('connection lost: '+e.message));return;}
      finish(level==='client-authentication'?new Error('authentication failed'):new SshConnectionError('could not connect: '+e.message));
    });
    connection.on('close',()=>finish(authError??(!ready?new SshConnectionError('connection closed before login'):!remoteExited?new ConnectionLost('connection lost'):undefined)));
    connection.on('ready',()=>{
      ready=true;
      const attached=(error:Error|undefined,stream:ClientChannel)=>{
        if(error){finish(error);return;}channel=stream;terminal.remote();
        stream.on('data',output);stream.stderr.on('data',output);stdout.on('drain',drain);
        stream.on('drain',()=>stdin.resume());
        stream.on('exit',(code:number|null,signal?:string)=>{remoteExited=true;exitCode=code??(signal?128+(constants.signals[signal as keyof typeof constants.signals]??0):0);});
        stream.on('error',(e:Error)=>finish(remoteExited?undefined:new ConnectionLost('connection lost: '+e.message)));
        stream.on('close',()=>finish(remoteExited?undefined:new ConnectionLost('connection lost')));
        stdin.on('data',input);stdin.once('end',endInput);stdin.on('error',localError);
        stdout.on('resize',resize);stdout.on('error',localError);stdin.resume();
        options.onReady?.();if(stdin.readableEnded)endInput();
      };
      const pty=remotePty(stdout.columns||80,stdout.rows||24),env=remoteEnvironment();
      if(profile.remoteCwd)connection.exec(remoteCommand(profile.remoteCwd,terminal.color),{pty,env},attached);
      else connection.shell(pty,{env},attached);
    });
    // No idle timeout. TCP keepalive is OS-level dead-peer detection; SSH
    // keepalives also keep NAT mappings active without a missed-reply cutoff.
    const socket=createConnection({host:profile.host,port:profile.port,...(profile.connectionOptions?.addressFamily?{family:profile.connectionOptions.addressFamily}:{})});
    socket.setKeepAlive(true,30000);socket.setTimeout(0);options.onSocket?.(socket);
    connection.connect({sock:socket,host:profile.host,port:profile.port,username,...(forward?{agent:forward.agent,agentForward:true}:{}),keepaliveInterval:options.keepaliveIntervalMs??30000,
      // ssh2 tests ++missed > countMax; zero means immediate disconnection,
      // not disabled. Infinity deliberately disables that liveness deadline.
      keepaliveCountMax:Infinity,readyTimeout:120000,
      hostVerifier:(key:Buffer,verify:(trusted:boolean)=>void)=>{void trustHost(userData,profile.host,profile.port,key,async info=>{
        stdout.write('  '+s.dim(info.type+' '+info.fingerprint)+'\n');return /^y(?:es)?$/i.test((await prompt('Trust this host? [y/N] ')).trim());
      }).then(trusted=>{if(!trusted)authError=new Error('host key was not trusted');verify(trusted);}).catch(e=>{authError=e;verify(false);});},
      authHandler:(left,_partial,next)=>{void(async()=>{
        while(index<methods.length){const method=methods[index++];if(!left||left.includes(method.type)||method.type==='agent'&&left.includes('publickey')){next(method);return;}}
        if(passwordTries<3&&(!left||left.includes('password'))){passwordTries++;next({type:'password',username,password:await prompt(`${username}@${profile.host}'s password: `,true)});return;}
        (next as unknown as (value:false)=>void)(false);
      })().catch(e=>{authError=e;connection.destroy();});},
    });
  });
}
