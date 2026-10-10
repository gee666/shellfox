import { Server, utils, type ServerChannel } from 'ssh2';
import { generateKeyPairSync } from 'node:crypto';
export interface SshFixtureRecord { auth:string[]; pty:unknown[]; commands:string[]; windows:unknown[]; bytes:Buffer[]; keepalives:number; shells:number; env:{key:string;val:string}[] }
export async function sshFixture(port=0){
  const hostKey=generateKeyPairSync('rsa',{modulusLength:2048}).privateKey.export({type:'pkcs1',format:'pem'}).toString();
  const records:SshFixtureRecord={auth:[],pty:[],commands:[],windows:[],bytes:[],keepalives:0,shells:0,env:[]};
  const settings={denyAuth:false,stallKeepalives:false,colors:false,rejectEnv:false};
  const heldReplies:(()=>void)[]=[];
  const clients=new Set<import('ssh2').Connection>();
  const server=new Server({hostKeys:[hostKey]},client=>{
    clients.add(client);client.on('close',()=>clients.delete(client));client.on('error',()=>{});
    // ssh2 automatically rejects keepalive global requests. Delay those replies
    // inside the fixture to emulate a stalled SSH process, not a dead TCP peer.
    const protocol=(client as unknown as {_protocol:{requestFailure:()=>void}})._protocol;
    const reply=protocol.requestFailure.bind(protocol);
    protocol.requestFailure=()=>{records.keepalives++;if(settings.stallKeepalives)heldReplies.push(reply);else reply();};
    client.on('authentication',ctx=>{
      records.auth.push(ctx.method);
      if(settings.denyAuth){ctx.reject(['publickey','password','keyboard-interactive']);return;}
      if(ctx.username==='fixture'&&ctx.method==='password'&&ctx.password==='fixture-secret')ctx.accept();
      else if(ctx.username==='fixture'&&ctx.method==='publickey'){
        const key=utils.parseKey(ctx.key.data);
        if(!(key instanceof Error)&&!Array.isArray(key)&&(!ctx.signature||key.verify(ctx.blob!,ctx.signature,ctx.hashAlgo)===true))ctx.accept();else ctx.reject();
      }else ctx.reject(['publickey','password','keyboard-interactive']);
    });
    client.on('ready',()=>client.on('session',accept=>{
      const session=accept();
      session.on('env',(ok,reject,info)=>{records.env.push(info);if(settings.rejectEnv)reject?.();else ok?.();});
      session.on('pty',(ok,_reject,info)=>{records.pty.push(info);ok?.();});
      session.on('window-change',(ok,_reject,info)=>{records.windows.push(info);ok?.();});
      const shell=(stream:ServerChannel)=>{
        if(settings.colors)stream.write('\x1b[31mRED\x1b[0m\x1b[38;5;202mINDEXED\x1b[0m\x1b[38;2;12;34;56mRGB\x1b[0m\r\n');
        stream.write('REMOTE READY\r\n');
        stream.on('data',(data:Buffer)=>{
          if(data.toString()==='DROP'){(client as unknown as {_sock:import('node:net').Socket})._sock.destroy();return;}
          records.bytes.push(Buffer.from(data));stream.write('REMOTE BYTES '+data.toString('hex')+'\r\n');
          if(data.includes(4)){stream.exit(7);stream.end();}
        });
        stream.on('end',()=>{stream.exit(7);stream.end();});stream.on('error',()=>{});
      };
      session.on('shell',acceptShell=>{records.shells++;shell(acceptShell());});
      session.on('exec',(acceptExec,_reject,info)=>{records.commands.push(info.command);shell(acceptExec());});
    }));
  });
  await new Promise<void>((resolve,reject)=>{server.once('error',reject);server.listen(port,'0.0.0.0',resolve);});
  return {server,hostKey,records,settings,flushKeepalives:()=>{for(const reply of heldReplies.splice(0))reply();},port:(server.address() as import('node:net').AddressInfo).port,
    close:async()=>{heldReplies.length=0;for(const c of clients)(c as unknown as {_sock:import('node:net').Socket})._sock.destroy();await new Promise<void>(resolve=>server.close(()=>resolve()));}};
}
