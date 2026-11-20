import {useCallback,useEffect,useRef,useState} from 'react';
import {AppState} from 'react-native';
import {v4 as uuid} from 'uuid';
import type {EngineSelection} from '../availability';
import {createProfileClient} from '../observer-v2/factory';
import type {ObserverSnapshot} from '../observer-v2/catalog';
import type {ManagerV2Client,ManagerState} from './client';

type Engine=Extract<EngineSelection,{engine:'rust-native'}>;
export interface ManagerHost {peerId:string;client:ManagerV2Client;state:ManagerState;snapshot:ObserverSnapshot|null;confirmed:boolean}
export function useManagerHosts(engine:Engine,report:(cause:unknown)=>void,onPaired?:()=>void){
  const clients=useRef(new Map<string,ManagerV2Client>());
  const pending=useRef(new Map<string,Promise<ManagerV2Client>>());
  const retirements=useRef(new Map<string,Promise<void>>());
  const disposers=useRef(new Map<ManagerV2Client,()=>void>());
  const connectedPeers=useRef(new Set<string>());
  const active=useRef(true),foreground=useRef(AppState.currentState==='active');
  const generation=useRef(0),appEpoch=useRef(0),stopping=useRef(false);
  const [hosts,setHosts]=useState<ManagerHost[]>([]);
  const refresh=useCallback(async(client:ManagerV2Client)=>{
    const binding=client.getState().binding;if(!binding)return;
    const reply=await client.request('getSnapshot',{});if(!reply.ok)throw new Error(reply.error.message);
    if(active.current&&client.getState().binding?.connectionId===binding.connectionId)
      setHosts(items=>items.map(item=>item.client===client?{...item,snapshot:reply.value}:item));
  },[]);
  const connect=useCallback(async(peerId:string,pairHost=true):Promise<ManagerV2Client>=>{
    if(stopping.current||!foreground.current)throw new Error('Manager network is stopping or backgrounded.');
    const cycle=generation.current,epoch=appEpoch.current;
    const existing=pending.current.get(peerId);if(existing)return existing;
    const valid=(client?:ManagerV2Client)=>active.current&&!stopping.current&&foreground.current&&cycle===generation.current&&epoch===appEpoch.current&&(!client||clients.current.get(peerId)===client);
    const requireCurrent=(client?:ManagerV2Client)=>{if(!valid(client))throw new Error('Manager connection lifecycle changed.');};
    const operation=(async()=>{
      let client=clients.current.get(peerId);
      const retirement=retirements.current.get(peerId);
      if(retirement){
        await retirement;requireCurrent(client);
        if(!client)throw new Error('Original retiring client was lost.');
        await client.reconnect(uuid());requireCurrent(client);
        if(retirements.current.get(peerId)===retirement)retirements.current.delete(peerId);
      }
      if(!client){
        const selected=await createProfileClient(peerId,{createClientId:uuid},{profile:'manager-v2',engine});
        if(selected.profile!=='manager-v2')throw new Error('Manager profile was not selected.');
        client=selected.client;
        if(!valid()){await client.close();throw new Error('Manager connection was retired.');}
        clients.current.set(peerId,client);const owned=client;
        const state=client.subscribeState(value=>{if(active.current)setHosts(items=>items.map(item=>item.client===owned?{...item,state:value,snapshot:value.status==='connected'?item.snapshot:null}:item));});
        const events=client.subscribe(event=>{if(event.type==='changed')void refresh(owned).catch(report);});
        disposers.current.set(client,()=>{state();events();});
        setHosts(items=>[...items,{peerId,client:owned,state:owned.getState(),snapshot:null,confirmed:false}]);
      }
      requireCurrent(client);
      if(client.getState().status!=='connected'){
        if(!retirement&&!['disconnected','discovered'].includes(client.getState().status)){await client.reconnect(uuid());requireCurrent(client);}
        const discovered=await client.discover();requireCurrent(client);
        const available='kind'in discovered&&discovered.kind==='available',checked=client;
        setHosts(items=>items.map(item=>item.client===checked?{...item,confirmed:available}:item));
        if(!available){if(pairHost)throw new Error('This peer has no compatible Manager Native0 profile.');return client;}
        if(pairHost){
          const reply=await client.pair(uuid());requireCurrent(client);
          if(!reply.ok)throw new Error(reply.error.message);
          onPaired?.();
        }
      }
      if(pairHost){await refresh(client);requireCurrent(client);connectedPeers.current.add(peerId);}
      return client;
    })().finally(()=>{if(pending.current.get(peerId)===operation)pending.current.delete(peerId);});
    pending.current.set(peerId,operation);return operation;
  },[engine,refresh,report,onPaired]);
  const closeAll=useCallback(async():Promise<unknown[]>=>{
    stopping.current=true;generation.current++;appEpoch.current++;
    try{
      const closing=[...clients.current].map(([peer,client])=>{
        const work=(async()=>{await client.close();
          if(clients.current.get(peer)===client){clients.current.delete(peer);connectedPeers.current.delete(peer);retirements.current.delete(peer);}
          disposers.current.get(client)?.();disposers.current.delete(client);
        })();
        retirements.current.set(peer,work);return work;
      });
      const results=await Promise.allSettled([...closing,...pending.current.values(),...retirements.current.values()]);
      if(active.current)setHosts(items=>items.filter(item=>clients.current.get(item.peerId)===item.client));
      return results.flatMap(result=>result.status==='rejected'?[result.reason]:[]);
    }finally{stopping.current=false;}
  },[]);
  useEffect(()=>{
    active.current=true;foreground.current=AppState.currentState==='active';
    const subscription=AppState.addEventListener('change',state=>{
      if(state==='inactive')return;
      foreground.current=state==='active';const epoch=++appEpoch.current,cycle=generation.current;
      for(const [peer,client]of clients.current){
        if(state==='background'){
          if(retirements.current.has(peer)||!connectedPeers.current.has(peer))continue;
          const retirement=(async()=>{
            if(!active.current||cycle!==generation.current||clients.current.get(peer)!==client)throw new Error('Original background client was retired.');
            // The client closes admission synchronously and joins its original
            // PAIR/opening/view work before native disconnection.
            const reply=await client.disconnect();if(!reply.ok)throw new Error(reply.error.message);
          })();
          retirements.current.set(peer,retirement);void retirement.catch(report);
        }else if(state==='active'&&connectedPeers.current.has(peer)){
          const retirement=retirements.current.get(peer);
          void(async()=>{
            await retirement;
            if(!active.current||!foreground.current||epoch!==appEpoch.current||cycle!==generation.current||clients.current.get(peer)!==client)return;
            await connect(peer);
          })().catch(error=>{if(active.current&&foreground.current&&epoch===appEpoch.current&&cycle===generation.current)report(error);});
        }
      }
    });
    return()=>{
      active.current=false;appEpoch.current++;subscription.remove();
      for(const [peer,client]of clients.current)void client.close().then(()=>{
        if(clients.current.get(peer)===client)clients.current.delete(peer);
        disposers.current.get(client)?.();disposers.current.delete(client);
      }).catch(report);
    };
  },[connect,report]);
  const check=useCallback((peerId:string)=>connect(peerId,false),[connect]);
  return {hosts,connect,check,refresh,closeAll};
}
