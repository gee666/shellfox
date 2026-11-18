// @vitest-environment jsdom
import {act,cleanup,renderHook} from '@testing-library/react';
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {useManagerHosts} from './use-manager-hosts';
const probe=vi.hoisted(()=>({discover:vi.fn(),pair:vi.fn(),snapshot:vi.fn(),close:vi.fn(),disconnect:vi.fn(),reconnect:vi.fn(),report:vi.fn(),paired:vi.fn(),stateChanged:(_state:string)=>{}}));
vi.mock('react-native',()=>({AppState:{currentState:'active',addEventListener:(_event:string,handler:(state:string)=>void)=>{probe.stateChanged=handler;return{remove:()=>{}};}}}));
vi.mock('../../network',()=>({nativeNetwork:{}}));
vi.mock('../observer-v2/factory',()=>({createProfileClient:async()=>({profile:'manager-v2',client:{getState:()=>({status:'disconnected'}),subscribe:()=>()=>{},subscribeState:()=>()=>{},discover:probe.discover,pair:probe.pair,getSnapshot:probe.snapshot,close:probe.close,disconnect:probe.disconnect,reconnect:probe.reconnect}})}));
beforeEach(()=>{vi.clearAllMocks();probe.close.mockResolvedValue(undefined);probe.disconnect.mockResolvedValue({ok:true,value:{disconnected:true}});probe.reconnect.mockResolvedValue(undefined);});
afterEach(cleanup);
const engine={engine:'rust-native',accepted:{android:true,ios:false}} as const;
it('checks actual Manager discovery without PAIR/snapshot or foreground auto-pair of candidates',async()=>{
 probe.discover.mockResolvedValue({kind:'available'});
 const {result}=renderHook(()=>useManagerHosts(engine,probe.report));
 await act(async()=>{await result.current.check('authenticated-map-peer');});
 expect(probe.discover).toHaveBeenCalledOnce();expect(probe.pair).not.toHaveBeenCalled();expect(probe.snapshot).not.toHaveBeenCalled();
 expect(result.current.hosts[0]?.confirmed).toBe(true);
 await act(async()=>{probe.stateChanged('background');probe.stateChanged('active');});
 expect(probe.pair).not.toHaveBeenCalled();expect(probe.discover).toHaveBeenCalledOnce();
});
it('clears the prior approval error only after actual successful PAIR, not discovery or pending failure',async()=>{
 probe.discover.mockResolvedValue({kind:'available'});probe.pair.mockResolvedValue({ok:false,error:{message:'Waiting for approval in Shellfox on the host.'}});
 const {result}=renderHook(()=>useManagerHosts(engine,probe.report,probe.paired));
 await act(async()=>{await result.current.check('authenticated-map-peer');});expect(probe.paired).not.toHaveBeenCalled();
 await act(async()=>{await expect(result.current.connect('authenticated-map-peer')).rejects.toThrow('Waiting for approval');});
 expect(probe.paired).not.toHaveBeenCalled();
 probe.pair.mockResolvedValue({ok:true,value:{}});
 await act(async()=>{await result.current.connect('authenticated-map-peer');});expect(probe.paired).toHaveBeenCalledOnce();
});
it('foreground joins the original background retirement before any discovery or PAIR',async()=>{
 probe.discover.mockResolvedValue({kind:'available'});probe.pair.mockResolvedValue({ok:true,value:{}});
 const {result}=renderHook(()=>useManagerHosts(engine,probe.report));await act(async()=>{await result.current.connect('peer');});
 let release:(value:unknown)=>void=()=>{throw Error('No original retirement');};probe.disconnect.mockImplementation(()=>new Promise(resolve=>{release=resolve;}));
 await act(async()=>{probe.stateChanged('background');probe.stateChanged('active');});
 try {expect(probe.discover).toHaveBeenCalledTimes(1);expect(probe.pair).toHaveBeenCalledTimes(1);}
 finally {await act(async()=>{release({ok:true,value:{disconnected:true}});});}
 expect(probe.reconnect).toHaveBeenCalledOnce();expect(probe.pair).toHaveBeenCalledTimes(2);
});
it('background-active-background epoch changes do not resume under the late original future',async()=>{
 probe.discover.mockResolvedValue({kind:'available'});probe.pair.mockResolvedValue({ok:true,value:{}});
 const {result}=renderHook(()=>useManagerHosts(engine,probe.report));await act(async()=>{await result.current.connect('peer');});
 let release:(value:unknown)=>void=()=>{throw Error('No retirement');};probe.disconnect.mockImplementation(()=>new Promise(resolve=>{release=resolve;}));
 await act(async()=>{probe.stateChanged('background');probe.stateChanged('active');probe.stateChanged('background');await Promise.resolve();release({ok:true,value:{disconnected:true}});});
 expect(probe.disconnect).toHaveBeenCalledOnce();expect(probe.discover).toHaveBeenCalledTimes(1);expect(probe.pair).toHaveBeenCalledTimes(1);
});
it('an unknown background retirement is retained, not automatically retried or recertified',async()=>{
 probe.discover.mockResolvedValue({kind:'available'});probe.pair.mockResolvedValue({ok:true,value:{}});
 const {result}=renderHook(()=>useManagerHosts(engine,probe.report));await act(async()=>{await result.current.connect('peer');});
 probe.disconnect.mockRejectedValue(new Error('Original cleanup outcome unknown'));
 await act(async()=>{probe.stateChanged('background');});await act(async()=>{probe.stateChanged('active');});
 expect(probe.discover).toHaveBeenCalledTimes(1);expect(probe.pair).toHaveBeenCalledTimes(1);expect(probe.report).toHaveBeenCalled();
});
it('retains a failed original client close and refuses fresh discovery or binding',async()=>{
 probe.discover.mockResolvedValue({kind:'available'});probe.pair.mockResolvedValue({ok:true,value:{}});
 const {result}=renderHook(()=>useManagerHosts(engine,probe.report));await act(async()=>{await result.current.connect('peer');});
 probe.close.mockRejectedValue(new Error('BUSY original native ownership retained'));
 await act(async()=>{const errors=await result.current.closeAll();expect(errors.length).toBeGreaterThan(0);});
 expect(result.current.hosts).toHaveLength(1);
 await act(async()=>{await expect(result.current.connect('peer')).rejects.toThrow('BUSY');});
 expect(probe.discover).toHaveBeenCalledTimes(1);expect(probe.pair).toHaveBeenCalledTimes(1);
});
it('keeps incompatible candidates unconfirmed without PAIR or legacy fallback',async()=>{
 probe.discover.mockResolvedValue({kind:'incompatible'});
 const {result}=renderHook(()=>useManagerHosts(engine,probe.report));
 await act(async()=>{await result.current.check('authenticated-map-peer');});
 expect(result.current.hosts[0]?.confirmed).toBe(false);expect(probe.pair).not.toHaveBeenCalled();expect(probe.snapshot).not.toHaveBeenCalled();
});
