import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, Pressable, ScrollView, Text, View } from 'react-native';
import type { TabDto } from '../../src/shared/contracts';
import type { MachineView, ShellfoxModel } from './useShellfox';
import { Button, Note, useUi } from './ui';
import type { PromptConfig } from './Prompt';
import { TerminalView } from './terminal/TerminalView';
import { ActionMenu, type MenuConfig } from './ActionMenu';
import { tabDot } from '../../src/shared/terminal-status';
import { StatusIndicator } from './StatusIndicator';
import { ConnectionStatus } from './ConnectionStatus';
import { TerminalOperationError } from './terminal/errors';
import type { TerminalBridgeState } from './terminal/bridge';
import { isConnectionFailure } from './connection-status';
import { RemoteClientError } from './client';

export function SessionScreen({
  machine,
  sessionId,
  model,
  back,
  prompt,
}: {
  machine: MachineView;
  sessionId: string;
  model: ShellfoxModel;
  back: () => void;
  prompt: (value: PromptConfig) => void;
}) {
  const { colors, styles } = useUi();
  const session = machine.snapshot?.sessions.find((item) => item.id === sessionId);
  const tabs = session?.tabs ?? [];
  const [selected, setSelected] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuConfig | null>(null);
  const active =
    tabs.find((tab) => tab.id === selected) ?? tabs.find((tab) => tab.lifecycle !== 'closed') ?? tabs[0];
  const client = machine.client;
  const terminalApi = client.terminalApi;
  const subscribeTerminal = client.subscribeTerminal;
  const metadataCurrent = machine.snapshotConnectionId === machine.state.connectionId;
  const canAttach = metadataCurrent && active?.lifecycle === 'open' && active.terminalKind !== 'external-legacy';
  const terminalKey = `${machine.peer.id}/${sessionId}/${active?.id ?? ''}/${active?.generation ?? ''}/${machine.state.connectionId ?? ''}`;
  const currentTerminalKey = useRef(terminalKey);
  currentTerminalKey.current = terminalKey;
  const [terminalPhase, setTerminalPhase] = useState<{ key: string; state: TerminalBridgeState } | null>(null);
  const [terminalError, setTerminalError] = useState<{ key: string; message: string } | null>(null);
  const phase = terminalPhase?.key === terminalKey ? terminalPhase.state : canAttach || !metadataCurrent || active?.lifecycle === 'launching' ? 'attaching' : 'closed';
  const onTerminalState = useCallback((state: TerminalBridgeState) => {
    if (currentTerminalKey.current !== terminalKey) return;
    setTerminalPhase(previous => previous?.key === terminalKey && previous.state === state ? previous : { key: terminalKey, state });
    if (state === 'attached' || state === 'closed' || state === 'blocked') setTerminalError(previous => previous?.key === terminalKey ? null : previous);
  }, [terminalKey]);
  const onTerminalError = useCallback((error: Error) => {
    if (currentTerminalKey.current !== terminalKey) return;
    if (error instanceof TerminalOperationError) {
      if (isConnectionFailure(error.detail)) return;
      if (error.detail.code === 'TARGET_LOST' || error.detail.code === 'NOT_FOUND') { void model.refresh(client); return; }
    }
    setTerminalError({ key: terminalKey, message: error.message });
  }, [terminalKey, model.refresh, client]);
  useEffect(() => {
    if (machine.state.status !== 'connected') return;
    void client.request('setVisibleSessions', { sessionIds: [sessionId] });
    return () => {
      void client.request('setVisibleSessions', { sessionIds: [] });
    };
  }, [client, sessionId, machine.state.status, machine.state.connectionId]);
  const run = (operation: () => Promise<void>) => {
    void operation().catch(model.report);
  };
  const manageTab = (tab: TabDto) => {
    const index = tabs.findIndex((item) => item.id === tab.id);
    const move = (offset: number) =>
      run(async () => {
        const order = tabs.map((item) => item.id);
        const target = index + offset;
        if (index < 0 || target < 0 || target >= order.length) return;
        [order[index], order[target]] = [order[target], order[index]];
        const result = await client.request('reorderTabs', { sessionId, tabIds: order });
        if (!result.ok) throw new Error(result.error.message);
        await model.refresh(client);
      });
    setMenu({
      title: tab.title,
      detail: tab.cwd,
      actions: [
        {
          title: 'Rename',
          action: () =>
            prompt({
              title: 'Rename terminal',
              label: 'Name',
              initial: tab.title,
              submit: async (title) => {
                const result = await client.request('renameTab', { sessionId, tabId: tab.id, title });
                if (!result.ok) throw new Error(result.error.message);
                await model.refresh(client);
              },
            }),
        },
        ...(index > 0 ? [{ title: 'Move left', action: () => move(-1) }] : []),
        ...(index >= 0 && index < tabs.length - 1 ? [{ title: 'Move right', action: () => move(1) }] : []),
        {
          title: 'Close terminal...',
          danger: true,
          action: () =>
            Alert.alert(
              'Close remote terminal?',
              'This terminates the shell and its processes on the host.',
              [
                { text: 'Cancel', style: 'cancel' },
                {
                  text: 'Close terminal',
                  style: 'destructive',
                  onPress: () =>
                    run(async () => {
                      if (!tab.generation)
                        throw new Error('Terminal identity is unavailable. Reconnect first.');
                      const result = await client.request('closeTab', {
                        tabId: tab.id,
                        generation: tab.generation,
                      });
                      if (!result.ok) throw new Error(result.error.message);
                      await model.refresh(client);
                    }),
                },
              ],
            ),
        },
      ],
    });
  };
  const add = (profileId?: string) =>
    run(async () => {
      const previous = new Set(tabs.map(tab => tab.id));
      const added = await client.request('addTab', { sessionId, ...(profileId ? { profileId } : {}) });
      if (!added.ok) throw new RemoteClientError(added.error);
      const created = added.value.tabs.find(tab => !previous.has(tab.id) && tab.lifecycle !== 'closed');
      await model.refresh(client);
      if (created) setSelected(created.id);
    });
  const chooseProfile = () =>
    run(async () => {
      const result = await client.request('getTerminalProfiles', {});
      if (!result.ok) throw new RemoteClientError(result.error);
      const profiles = result.value.profiles.filter(profile => profile.available && !profile.elevation);
      setMenu({
        title: 'New terminal',
        detail: 'Choose a shell on the host.',
        actions: profiles.map(profile => ({ title: profile.label, action: () => add(profile.id) })),
      });
    });
  return (
    <View style={styles.page}>
      {menu && <ActionMenu menu={menu} close={() => setMenu(null)} />}
      <View style={styles.header}>
        <Button title="‹ Sessions" subtle onPress={back} />
        <View style={styles.grow}>
          <Text numberOfLines={1} style={styles.label}>
            {session?.title ?? 'Session'}
          </Text>
          <View style={[styles.row, { gap: 6 }]}>
            <Text numberOfLines={1} style={[styles.subtitle, styles.grow]}>{machine.peer.hostname}</Text>
            <ConnectionStatus model={model} peerId={machine.peer.id} terminalState={phase} />
          </View>
        </View>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="New terminal"
          accessibilityState={{ disabled: !session?.canAddTab || machine.state.status !== 'connected' }}
          disabled={!session?.canAddTab || machine.state.status !== 'connected'}
          accessibilityHint="Tap to add the default shell. Long press to choose a profile."
          onPress={() => add()}
          onLongPress={chooseProfile}
          style={({ pressed }) => ({
            width: 44, height: 44, borderRadius: 22, borderWidth: 1, borderColor: colors.line,
            alignItems: 'center', justifyContent: 'center',
            opacity: !session?.canAddTab || machine.state.status !== 'connected' ? 0.4 : pressed ? 0.65 : 1,
          })}
        >
          <Text style={{ color: colors.text, fontSize: 26, lineHeight: 30 }}>+</Text>
        </Pressable>
      </View>
      {machine.state.status !== 'connected' ? (
        <View style={styles.content}>
          {machine.state.status === 'awaiting-approval' && <Note>Approve this device on the host.</Note>}
          {machine.state.status === 'connecting' || machine.state.status === 'reconnecting' ? <StatusIndicator loading label="Connecting" /> : null}
          {machine.state.status === 'awaiting-approval'
            ? <Button title="Cancel request" onPress={() => run(() => model.disconnect(machine.peer.id))} />
            : <Button title="Reconnect" onPress={() => client.reconnect()} />}
        </View>
      ) : !session ? (
        <View style={styles.content}>
          <Note>This session is no longer available on the host.</Note>
        </View>
      ) : (
        <>
          <View style={{ borderBottomWidth: 1, borderColor: colors.line }}>
            <ScrollView
              horizontal
              showsHorizontalScrollIndicator={false}
              contentContainerStyle={{ paddingHorizontal: 8 }}
            >
              {tabs.map((tab) => {
                const indicator = tabDot(tab, machine.busyTabIds[tab.id]);
                return (
                <Pressable
                  key={tab.id}
                  accessibilityRole="tab"
                  accessibilityLabel={tab.title}
                  accessibilityState={{ selected: active?.id === tab.id }}
                  onPress={() => setSelected(tab.id)}
                  onLongPress={() => manageTab(tab)}
                  style={{
                    paddingHorizontal: 12,
                    paddingVertical: 10,
                    minHeight: 44,
                    flexDirection: 'row',
                    alignItems: 'center',
                    gap: 7,
                    borderBottomWidth: 2,
                    borderColor: active?.id === tab.id ? colors.accent : 'transparent',
                  }}
                >
                  <StatusIndicator kind={indicator.kind} loading={!metadataCurrent} label={metadataCurrent ? indicator.title : 'Loading terminal'} />
                  <Text style={{ color: active?.id === tab.id ? colors.text : colors.muted }}>
                    {tab.title}
                  </Text>
                </Pressable>
                );
              })}
            </ScrollView>
          </View>
          {canAttach && terminalError?.key === terminalKey && <View style={[styles.row, { paddingHorizontal: 12, paddingVertical: 6 }]}><View style={styles.grow}><Note error>{terminalError.message}</Note></View><Button title="Dismiss" subtle onPress={() => setTerminalError(null)} /></View>}
          {canAttach && active ? (
            <TerminalView
              key={`${active.id}/${active.generation ?? ''}`}
              api={terminalApi}
              subscribeTerminal={subscribeTerminal}
              tabId={active.id}
              fontSize={model.preferences.fontSize}
              connectionEpoch={machine.state.connectionId ?? ''}
              onError={onTerminalError}
              onState={onTerminalState}
            />
          ) : (
            <View style={styles.content}>
              {!metadataCurrent || active?.lifecycle === 'launching' ? <StatusIndicator loading label="Opening terminal" /> : active?.terminalKind === 'external-legacy' ? <Note>External terminal</Note> : null}
            </View>
          )}
        </>
      )}
    </View>
  );
}
