import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {ManagerV2Client} from './client';
import {ManagerLifecycleNative,CLIENT,ATTEMPT,TAB} from './test-native';
const clients:ManagerV2Client[]=[];
beforeEach(()=>vi.useFakeTimers());
afterEach(async()=>{for(const client of clients.splice(0)){const closing=client.close().catch(()=>undefined);await vi.advanceTimersByTimeAsync(500);await closing;}vi.useRealTimers();});
async function connected(){const native=new ManagerLifecycleNative(),client=await ManagerV2Client.create(native,'inert-lifecycle-fixture',CLIENT);clients.push(client);const discovery=client.discover();await vi.advanceTimersByTimeAsync(150);await discovery;const pair=client.pair(ATTEMPT);await vi.advanceTimersByTimeAsync(150);await pair;return{native,client};}
async function opened(client:ManagerV2Client){const opening=client.openTerminal(TAB,true,{foreground:'#ffffff',background:'#000000'});await vi.advanceTimersByTimeAsync(250);return opening;}
it('joins the captured original PAIR before invalidating its binding for retirement',async()=>{
 const native=new ManagerLifecycleNative(),client=await ManagerV2Client.create(native,'inert-lifecycle-fixture',CLIENT);clients.push(client);
 const discovery=client.discover();await vi.advanceTimersByTimeAsync(150);await discovery;
 let release:()=>void=()=>{throw Error('No original pair');};native.pairBarrier=new Promise<void>(resolve=>{release=resolve;});
 const pairing=client.pair(ATTEMPT);await vi.advanceTimersByTimeAsync(100);const retirement=client.disconnect();
 try {expect(native.disconnectManagerV2).not.toHaveBeenCalled();}finally{release();}
 await vi.advanceTimersByTimeAsync(400);await pairing;await retirement;expect(native.disconnectManagerV2).toHaveBeenCalledOnce();
});
it('detaches and closes the original view before native disconnect invalidates its binding',async()=>{
 const {native,client}=await connected();await opened(client);native.calls.length=0;
 let release:()=>void=()=>{throw Error('No detach barrier');};native.detachBarrier=new Promise<void>(resolve=>{release=resolve;});
 const disconnecting=client.disconnect();expect(client.disconnect()).toBe(disconnecting);await vi.advanceTimersByTimeAsync(150);
 try {expect(native.calls).toEqual(['detachTerminal']);expect(native.disconnectManagerV2).not.toHaveBeenCalled();}
 finally {release();}
 await vi.advanceTimersByTimeAsync(300);await disconnecting;
 expect(native.calls).toEqual(['detachTerminal','disconnect']);expect(native.unbindTerminal).toHaveBeenCalledOnce();expect(native.closeTerminal).toHaveBeenCalledOnce();
});
it('joins a late original opening and its cleanup before disconnect, with no new admission',async()=>{
 const {native,client}=await connected();let release:()=>void=()=>{throw Error('No attach barrier');};native.attachBarrier=new Promise<void>(resolve=>{release=resolve;});
 const opening=client.openTerminal(TAB,true,{foreground:'#ffffff',background:'#000000'});void opening.catch(()=>undefined);await vi.advanceTimersByTimeAsync(100);
 const disconnecting=client.disconnect();await vi.advanceTimersByTimeAsync(100);
 try {expect(native.disconnectManagerV2).not.toHaveBeenCalled();
 await expect(client.openTerminal('50000000-0000-4000-8000-000000000002',true,{foreground:'#ffffff',background:'#000000'})).rejects.toThrow();}
 finally {release();}
 await vi.advanceTimersByTimeAsync(500);await opening;await disconnecting;
 expect(native.calls).toEqual(['attachTerminal','detachTerminal','disconnect']);
});
it('joins one original Surface.close promise and retains unknown cleanup without disconnect or replay',async()=>{
 const {native,client}=await connected();const view=await opened(client);native.calls.length=0;
 native.unbindTerminal.mockRejectedValueOnce(new Error('BUSY original cleanup'));
 const first=view.close(),second=view.close();expect(second).toBe(first);
 const failed=expect(first).rejects.toThrow('BUSY');await vi.advanceTimersByTimeAsync(250);await failed;
 const disconnectFailed=expect(client.disconnect()).rejects.toThrow('BUSY');await vi.advanceTimersByTimeAsync(250);await disconnectFailed;
 expect(native.disconnectManagerV2).not.toHaveBeenCalled();expect(native.unbindTerminal).toHaveBeenCalledOnce();expect(native.closeTerminal).not.toHaveBeenCalled();
 await expect(view.close()).rejects.toThrow('BUSY');expect(native.unbindTerminal).toHaveBeenCalledOnce();
 await expect(client.close()).rejects.toThrow('BUSY');expect(native.closeClient).not.toHaveBeenCalled();
});
it('retains a late unknown opening without unbind, native disconnect or fresh reconnect',async()=>{
 const {native,client}=await connected();let release:()=>void=()=>{throw Error('No original opening');};native.attachBarrier=new Promise<void>(resolve=>{release=resolve;});native.attachStatus=9;
 const opening=client.openTerminal(TAB,true,{foreground:'#ffffff',background:'#000000'});
 const rejectedOpening=expect(opening).rejects.toMatchObject({status:9});
 await vi.advanceTimersByTimeAsync(100);const disconnecting=client.disconnect();const rejectedDisconnect=expect(disconnecting).rejects.toMatchObject({status:9});
 release();await vi.advanceTimersByTimeAsync(300);await rejectedOpening;await rejectedDisconnect;
 expect(native.unbindTerminal).not.toHaveBeenCalled();expect(native.closeTerminal).not.toHaveBeenCalled();expect(native.disconnectManagerV2).not.toHaveBeenCalled();
 await expect(client.reconnect('20000000-0000-4000-8000-000000000002')).rejects.toMatchObject({status:9});expect(native.reconnectManagerV2).not.toHaveBeenCalled();
});
it('keeps explicit poll native error status without turning it into success or a new pair',async()=>{
 const {native,client}=await connected();native.pollStatus=9;
 const poll=client.poll();const failed=expect(poll).rejects.toMatchObject({status:9});await vi.advanceTimersByTimeAsync(150);await failed;
 expect(native.pairManagerV2).toHaveBeenCalledTimes(1);expect(native.reconnectManagerV2).not.toHaveBeenCalled();
});
