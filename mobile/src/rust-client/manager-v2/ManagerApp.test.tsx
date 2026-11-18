// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { tabSchema } from '../observer-v2/catalog';
import { ManagerApp, managerReadOnlyIntent } from './ManagerApp';
const probe=vi.hoisted(()=>{const peers:{id:string;hostname:string;online:boolean;addresses:string[]}[]=[];const hosts:{peerId:string;confirmed:boolean}[]=[];return {peers,hosts,error:null as string|null,setError:vi.fn(),pairSucceeded:()=>{},check:vi.fn(),connect:vi.fn(),closeAll:vi.fn(),stopNetwork:vi.fn(),report:vi.fn(),retryStart:vi.fn()};});
vi.mock('react-native',()=>({View:({children}:{children?:ReactNode})=><div>{children}</div>,Text:({children}:{children?:ReactNode})=><span>{children}</span>,ScrollView:({children}:{children?:ReactNode})=><div>{children}</div>}));
vi.mock('react-native-safe-area-context',()=>({SafeAreaView:({children}:{children:ReactNode})=><div aria-label="safe area">{children}</div>}));
vi.mock('../../theme',()=>({ThemeProvider:({children}:{children:ReactNode})=><>{children}</>}));
vi.mock('../../ui',()=>({useUi:()=>({styles:{}}),Card:({children}:{children?:ReactNode})=><div>{children}</div>,Note:({children}:{children?:ReactNode})=><span>{children}</span>,Button:({title,onPress,disabled}:{title:string;onPress:()=>void;disabled?:boolean})=><button disabled={disabled} onClick={onPress}>{title}</button>}));
vi.mock('../../terminal/RustTerminalView',()=>({RustTerminalView:()=> <div aria-label="native terminal test probe"/>}));
vi.mock('../../useShellfox',()=>({useShellfox:()=>({preferences:{fontSize:14},report:probe.report,network:{status:'online'},ready:true,peers:probe.peers,error:probe.error,setError:probe.setError,stopNetwork:probe.stopNetwork,retryStart:probe.retryStart})}));
vi.mock('./use-manager-hosts',()=>({useManagerHosts:(_engine:unknown,_report:unknown,onPaired:()=>void)=>{probe.pairSucceeded=onPaired;return {hosts:probe.hosts,connect:probe.connect,check:probe.check,refresh:vi.fn(),closeAll:probe.closeAll};}}));
beforeEach(()=>{vi.clearAllMocks();probe.peers.length=0;probe.hosts.length=0;probe.error=null;probe.setError.mockImplementation(value=>{probe.error=value;});probe.check.mockResolvedValue(undefined);probe.stopNetwork.mockResolvedValue(undefined);});
afterEach(cleanup);
const engine={engine:'rust-native',accepted:{android:true,ios:false}} as const;
it('removes the stale approval label when the host hook reports actual PAIR success',async()=>{
 probe.error='Waiting for approval in Shellfox on the host.';const view=render(<ManagerApp engine={engine}/>);
 expect(screen.getByText(probe.error)).toBeTruthy();expect(probe.setError).not.toHaveBeenCalled();
 await act(async()=>{probe.pairSucceeded();});expect(probe.setError).toHaveBeenCalledWith(null);
 view.rerender(<ManagerApp engine={engine}/>);expect(screen.queryByText('Waiting for approval in Shellfox on the host.')).toBeNull();
});
it.each(['notRecorded','pending','unavailable','invalidEvidence'])('unknown %s metadata expresses control intent without changing source facts',reason=>{
 const kind=tabSchema.shape.sourceKind.parse({state:'unknown',reason});const before=JSON.stringify(kind);
 expect(managerReadOnlyIntent(kind)).toBe(false);expect(JSON.stringify(kind)).toBe(before);
});
it.each(['automationPipe','externalBytes'])('keeps known %s sources read-only',value=>{
 expect(managerReadOnlyIntent(tabSchema.shape.sourceKind.parse({state:'known',value}))).toBe(true);
});
it('requests control for a known PTY without manufacturing authority',()=>{
 expect(managerReadOnlyIntent(tabSchema.shape.sourceKind.parse({state:'known',value:'pty'}))).toBe(false);
});
it('offers Connect only after Manager discovery confirms an authenticated map candidate',async()=>{
 probe.peers.push({id:'actual-map-id',hostname:'candidate',online:true,addresses:[]});probe.hosts.push({peerId:'actual-map-id',confirmed:false});
 const view=render(<ManagerApp engine={engine}/>);
 await act(async()=>{});expect(probe.check).toHaveBeenCalledWith('actual-map-id');expect(probe.connect).not.toHaveBeenCalled();
 expect(screen.queryByRole('button',{name:'Connect'})).toBeNull();expect(screen.getByRole('button',{name:'Check host'})).toBeTruthy();
 expect(screen.getByText('actual-map-id')).toBeTruthy();probe.hosts[0]!.confirmed=true;view.rerender(<ManagerApp engine={engine}/>);
 expect(screen.getByRole('button',{name:'Connect'})).toBeTruthy();expect(probe.connect).not.toHaveBeenCalled();
});
it('waits for actual view/request retirement before native network stop',async()=>{
 let retire:(value:unknown[])=>void=()=>{throw new Error('retirement not started');};
 probe.closeAll.mockImplementation(()=>new Promise<unknown[]>(resolve=>{retire=resolve;}));
 render(<ManagerApp engine={engine}/>);fireEvent.click(screen.getByRole('button',{name:'Stop network'}));
 expect(probe.closeAll).toHaveBeenCalledOnce();expect(probe.stopNetwork).not.toHaveBeenCalled();
 await act(async()=>{retire([]);});expect(probe.stopNetwork).toHaveBeenCalledOnce();expect(probe.report).not.toHaveBeenCalled();
});
it('stops the node but reports uncertain remote cleanup without pretending releases succeeded',async()=>{
 probe.closeAll.mockResolvedValue([new Error('release unconfirmed')]);render(<ManagerApp engine={engine}/>);
 await act(async()=>{fireEvent.click(screen.getByRole('button',{name:'Stop network'}));});
 expect(probe.stopNetwork).toHaveBeenCalledOnce();expect(probe.report).toHaveBeenCalledOnce();
});
