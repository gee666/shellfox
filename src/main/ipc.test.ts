import { beforeEach, it, expect, vi } from 'vitest';
import type { BrowserWindow, IpcMainInvokeEvent } from 'electron';
import type { SessionService } from './service';
const state=vi.hoisted(()=>({handler:undefined as undefined | ((event: unknown,request: unknown)=>Promise<unknown>),removed:false}));
vi.mock('electron',()=>({
  ipcMain:{handle:(_channel:string,fn:typeof state.handler)=>{state.handler=fn;},removeHandler:()=>{state.removed=true;}},
  dialog:{showOpenDialog:async()=>({canceled:true,filePaths:[]})},
}));
import { installIpc, isTrustedSender } from './ipc';
const url='file:///C:/app/index.html';
function fixture(){
  const frame={url};
  const contents={mainFrame:frame,send:vi.fn()};
  const window={webContents:contents,isDestroyed:()=>false,isFocused:()=>true} as unknown as BrowserWindow;
  const event={sender:contents,senderFrame:frame} as unknown as IpcMainInvokeEvent;
  const service={backend:{authorizeForegroundFocus:vi.fn()},activateSession:vi.fn(async()=>({ok:false,error:{code:'FOCUS_DENIED',message:'denied',retryable:true}})),getSnapshot:vi.fn(()=>({ok:true,value:{secret:'do not expose'}})),subscribe:vi.fn(()=>vi.fn())} as unknown as SessionService;
  return {window,event,service};
}
beforeEach(()=>{state.handler=undefined;state.removed=false;});
it('accepts only the exact app webContents, main frame, and URL',()=>{
  const {window,event}=fixture();
  expect(isTrustedSender(event,window,url)).toBe(true);
  expect(isTrustedSender({...event,senderFrame:{url}} as unknown as IpcMainInvokeEvent,window,url)).toBe(false);
  expect(isTrustedSender({...event,sender:{}} as unknown as IpcMainInvokeEvent,window,url)).toBe(false);
  expect(isTrustedSender(event,window,'https://evil.invalid')).toBe(false);
});
it('rejects unauthorized requests before touching services',async()=>{
  const {window,event,service}=fixture(); installIpc(window,url,service);
  expect(await state.handler!({...event,sender:{}},{version:1,method:'getSnapshot',payload:{}})).toMatchObject({ok:false,error:{code:'AUTH_FAILED'}});
  expect(service.getSnapshot).not.toHaveBeenCalled();
});
it.each([
  {version:2,method:'getSnapshot',payload:{}},
  {version:1,method:'run-command',payload:{command:'bad'}},
  {version:1,method:'getSnapshot',payload:{channel:'native:launch'}},
  {version:1,method:'getSnapshot',payload:{},extra:true},
])('rejects malformed envelopes/payloads %j',async request=>{
  const {window,event,service}=fixture(); installIpc(window,url,service);
  expect(await state.handler!(event,request)).toMatchObject({ok:false,error:{code:'VALIDATION'}});
  expect(service.getSnapshot).not.toHaveBeenCalled();
});
it('validates outgoing DTOs without sending private fields',async()=>{
  const {window,event,service}=fixture(); const dispose=installIpc(window,url,service);
  const response=await state.handler!(event,{version:1,method:'getSnapshot',payload:{}});
  expect(response).toMatchObject({ok:false,error:{code:'INTERNAL'}});
  expect(JSON.stringify(response)).not.toContain('secret');
  dispose(); expect(state.removed).toBe(true);
});
it('authorizes foreground delegation only for a validated focused app request',async()=>{
 const {window,event,service}=fixture();installIpc(window,url,service);
 const request={version:1,method:'activateSession',payload:{sessionId:'00000000-0000-4000-8000-000000000001'}};
 await state.handler!(event,request);expect((service.backend as any).authorizeForegroundFocus).toHaveBeenCalledTimes(1);
 (window as any).isFocused=()=>false;await state.handler!(event,request);expect((service.backend as any).authorizeForegroundFocus).toHaveBeenCalledTimes(1);
 await state.handler!({...event,sender:{}},request);expect((service.backend as any).authorizeForegroundFocus).toHaveBeenCalledTimes(1);
});
it('directory cancel returns null',async()=>{
  const {window,event,service}=fixture(); installIpc(window,url,service);
  expect(await state.handler!(event,{version:1,method:'chooseDirectory',payload:{}})).toEqual({ok:true,value:null});
});
it('routes deleteSession only with a strict session id payload and validates the response',async()=>{
  const {window,event,service}=fixture();
  (service as any).deleteSession=vi.fn(async()=>({ok:true,value:{deleted:true}}));
  installIpc(window,url,service);
  const sessionId='00000000-0000-4000-8000-000000000001';
  expect(await state.handler!(event,{version:1,method:'deleteSession',payload:{sessionId}})).toEqual({ok:true,value:{deleted:true}});
  expect(service.deleteSession).toHaveBeenCalledWith({sessionId});
  for(const payload of [{},{sessionId:'x'},{sessionId,force:true}])
    expect(await state.handler!(event,{version:1,method:'deleteSession',payload})).toMatchObject({ok:false,error:{code:'VALIDATION'}});
  expect(service.deleteSession).toHaveBeenCalledTimes(1);
  (service as any).deleteSession=vi.fn(async()=>({ok:true,value:{deleted:false}}));
  expect(await state.handler!(event,{version:1,method:'deleteSession',payload:{sessionId}})).toMatchObject({ok:false,error:{code:'INTERNAL'}});
  await state.handler!({...event,sender:{}},{version:1,method:'deleteSession',payload:{sessionId}});
  expect(service.deleteSession).toHaveBeenCalledTimes(1);
});
