import { Server, utils, type ServerChannel } from 'ssh2';
import { generateKeyPairSync } from 'node:crypto';
import type { AddressInfo, Socket } from 'node:net';

const loopback = '127.0.0.1';
export interface SshFixtureRecord { auth:string[]; pty:unknown[]; commands:string[]; windows:unknown[]; bytes:Buffer[]; keepalives:number; shells:number; env:{key:string;val:string}[] }
export async function sshFixture(publicKeys: readonly Buffer[] = []){
  const hostKey=generateKeyPairSync('rsa',{modulusLength:2048}).privateKey.export({type:'pkcs1',format:'pem'}).toString();
  const records:SshFixtureRecord={auth:[],pty:[],commands:[],windows:[],bytes:[],keepalives:0,shells:0,env:[]};
  const settings={denyAuth:false,stallKeepalives:false,colors:false,rejectEnv:false};
  const heldReplies:(()=>void)[]=[];
  const sockets=new Map<Socket, Promise<void>>();
  const server=new Server({hostKeys:[hostKey]},client=>{
    const socket = (client as unknown as {_sock: Socket})._sock;
    sockets.set(socket, new Promise<void>(resolve => socket.once('close', () => { sockets.delete(socket); resolve(); })));
    client.on('error',()=>{});
    // ssh2 automatically rejects keepalive global requests. Delay those replies
    // inside the fixture to emulate a stalled SSH process, not a dead TCP peer.
    const protocol=(client as unknown as {_protocol:{requestFailure:()=>void}})._protocol;
    const reply=protocol.requestFailure.bind(protocol);
    protocol.requestFailure=()=>{records.keepalives++;if(settings.stallKeepalives)heldReplies.push(reply);else reply();};
    client.on('authentication',ctx=>{
      records.auth.push(ctx.method);
      if(settings.denyAuth){ctx.reject(['publickey','password','keyboard-interactive']);return;}
      if(ctx.username==='fixture'&&ctx.method==='password'&&ctx.password==='fixture-secret')ctx.accept();
      else if(ctx.username==='fixture'&&ctx.method==='publickey'&&publicKeys.some(key=>key.equals(ctx.key.data))){
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
          if(data.toString()==='DROP'){socket.destroy();return;}
          records.bytes.push(Buffer.from(data));stream.write('REMOTE BYTES '+data.toString('hex')+'\r\n');
          if(data.includes(4)){stream.exit(7);stream.end();}
        });
        stream.on('end',()=>{stream.exit(7);stream.end();});stream.on('error',()=>{});
      };
      session.on('shell',acceptShell=>{records.shells++;shell(acceptShell());});
      session.on('exec',(acceptExec,_reject,info)=>{records.commands.push(info.command);shell(acceptExec());});
    }));
  });
  const listen = (port: number) => new Promise<void>((resolve, reject) => {
    const failed = (error: Error) => reject(error);
    server.once('error', failed);
    server.listen(port, loopback, () => { server.off('error', failed); resolve(); });
  });
  await listen(0);
  const port = (server.address() as AddressInfo).port;
  return {hostKey,records,settings,flushKeepalives:()=>{for(const reply of heldReplies.splice(0))reply();},port,
    rebind: () => listen(port),
    close:async()=>{
      heldReplies.length=0;
      const closed = server.address() ? new Promise<void>(resolve=>server.close(()=>resolve())) : Promise.resolve();
      const joined = [...sockets.values()];
      for (const socket of sockets.keys()) socket.destroy();
      await Promise.all([closed, ...joined]);
    }};
}
