import React, { useMemo, useState, useEffect, useRef, useCallback } from 'react';
import { ScrollView, Text, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useShellfox } from '../../useShellfox';
import { ThemeProvider } from '../../theme';
import { Button, Card, Note, useUi } from '../../ui';
import { RustTerminalView } from '../../terminal/RustTerminalView';
import type { ObserverTab } from '../observer-v2/catalog';
import type { EngineSelection } from '../availability';
import { useManagerHosts } from './use-manager-hosts';

// Unknown metadata may request IfFree. Only the Service can admit creation or
// control; this does not turn metadata into PTY or launch-eligibility evidence.
export function managerReadOnlyIntent(kind:ObserverTab['sourceKind']):boolean {
  return kind.state==='known'&&kind.value!=='pty';
}
type NativeEngine=Extract<EngineSelection,{engine:'rust-native'}>;
export function ManagerApp({engine,controlURL}:{engine:NativeEngine;controlURL?:string}) {
  const model=useShellfox(false,controlURL);
  return <ThemeProvider accentColor={model.preferences.accentColor} backgroundColor={model.preferences.backgroundColor}>
    <SafeAreaView style={{flex:1}} edges={['top','left','right']}><ManagerScreen model={model} engine={engine}/></SafeAreaView>
  </ThemeProvider>;
}
function ManagerScreen({model,engine}:{model:ReturnType<typeof useShellfox>;engine:NativeEngine}) {
  const {styles}=useUi();
  const pairSucceeded=useCallback(()=>model.setError(null),[model.setError]);
  const {hosts,connect,check,refresh,closeAll}=useManagerHosts(engine,model.report,pairSucceeded);
  const [peerId,setPeer]=useState<string|null>(null);
  const [tab,setTab]=useState<{id:string;readOnly:boolean}|null>(null);
  const [busy,setBusy]=useState(false);
  const checked=useRef(new Set<string>());
  useEffect(()=>{for(const peer of model.peers){if(peer.online&&!checked.current.has(peer.id)){checked.current.add(peer.id);void check(peer.id).catch(model.report);}}},[model.peers,check,model.report]);
  const host=hosts.find(value=>value.peerId===peerId);
  const driver=useMemo(()=>host?.client.terminals,[host?.client]);
  const run=(operation:()=>Promise<unknown>)=>{setBusy(true);void operation().catch(model.report).finally(()=>setBusy(false));};
  const stop=()=>run(async()=>{
    const cleanupErrors=await closeAll();setPeer(null);setTab(null);
    await model.stopNetwork();
    if(cleanupErrors.length)throw new AggregateError(cleanupErrors,'Network stopped; some remote releases were not confirmed.');
  });
  if(tab&&host&&driver&&host.state.status==='connected')return <View style={styles.page}>
    <View style={styles.header}><Button title="Back" subtle onPress={()=>setTab(null)}/><Text style={styles.title}>Terminal</Text><Button title="Stop network" subtle disabled={busy} onPress={stop}/></View>
    <RustTerminalView key={`${peerId}/${tab.id}/${host.state.binding?.connectionId}`} driver={driver} tabId={tab.id}
      readOnly={tab.readOnly} fontSize={model.preferences.fontSize} modifierReleaseDelayMs={model.preferences.modifierReleaseDelayMs}
      connectionEpoch={host.state.binding?.connectionId??''} onError={model.report}/>
  </View>;
  return <View style={styles.page}>
    <View style={styles.header}><Button title="Hosts" subtle onPress={()=>{setPeer(null);setTab(null);}}/><Text style={styles.title}>Manager</Text><Button title="Stop network" subtle disabled={busy||!model.ready} onPress={stop}/></View>
    <ScrollView contentContainerStyle={styles.content}>
      <Note>{model.network.status}</Note>
      {model.error&&<Note error>{model.error}</Note>}
      {!model.ready&&<Button title="Retry startup" disabled={busy} onPress={()=>run(model.retryStart)}/>}
      {model.network.status==='login-required'&&<Button title="Sign in" disabled={busy} onPress={()=>run(()=>model.login())}/>}
      {!peerId?model.peers.map(peer=><Card key={peer.id}>
        <Text style={styles.label}>{peer.hostname}</Text><Note>{peer.id}</Note>
        <Button title={hosts.some(host=>host.peerId===peer.id&&host.confirmed)?'Connect':'Check host'} disabled={busy||!peer.online} onPress={()=>run(async()=>{if(hosts.some(host=>host.peerId===peer.id&&host.confirmed)){await connect(peer.id);setPeer(peer.id);}else await check(peer.id);})}/>
      </Card>):<>
        <Note>{host?.state.status??'disconnected'}</Note>
        <Button title={host?.state.status==='connected'?'Refresh':'Connect'} disabled={busy}
          onPress={()=>run(()=>host?.state.status==='connected'?refresh(host.client):connect(peerId))}/>
        {host?.snapshot?.entries.map(entry=>entry.kind==='unavailable'?<Card key={entry.sessionId}><Note>Session unavailable</Note></Card>:<Card key={entry.value.id}>
          <Text style={styles.label}>{entry.value.title}</Text>
          {entry.value.tabs.map(value=><Button key={value.id} title={value.title||'Terminal'} subtle
            disabled={host.state.status!=='connected'} onPress={()=>setTab({id:value.id,readOnly:managerReadOnlyIntent(value.sourceKind)})}/>)}
        </Card>)}
      </>}
    </ScrollView>
  </View>;
}
