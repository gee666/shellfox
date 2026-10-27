import React, { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Alert, Pressable, SectionList, StyleSheet, Text, TextInput, View } from 'react-native';
import { v4 as uuid } from 'uuid';
import type { SessionDto } from '../../src/shared/contracts';
import { sessionDot, type DotState } from '../../src/shared/terminal-status';
import { sortVisibleSessions } from '../../src/shared/session-order';
import type { MachineView, ShellfoxModel } from './useShellfox';
import { Button, Note, useUi } from './ui';
import { StatusIndicator } from './StatusIndicator';
import type { PromptConfig } from './Prompt';
import { ActionMenu, type MenuConfig } from './ActionMenu';
import { EnvironmentEditor } from './EnvironmentEditor';
import { ConnectionStatus } from './ConnectionStatus';
import { isConnectionFailure } from './connection-status';
import { DirectoryPrompt, type DirectoryPromptConfig } from './DirectoryPrompt';

export interface OpenSession {
  peerId: string;
  sessionId: string;
}
export function SessionsScreen({
  model,
  open,
  prompt,
  settings,
}: {
  model: ShellfoxModel;
  open: (selection: OpenSession) => void;
  prompt: (config: PromptConfig) => void;
  settings: () => void;
}) {
  const { colors, styles } = useUi();
  const [searching, setSearching] = useState(false);
  const [search, setSearch] = useState('');
  const [menu, setMenu] = useState<MenuConfig | null>(null);
  const [environment, setEnvironment] = useState<{ client: MachineView['client']; session: SessionDto } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [directoryPrompt, setDirectoryPrompt] = useState<DirectoryPromptConfig | null>(null);
  const visibleIds = useRef(new Set<string>());
  const interests = useRef(new Map<MachineView['client'], { connectionId: string | null; ids: string }>());
  const syncVisibility = useRef(() => {});
  useEffect(() => {
    syncVisibility.current = () => {
      for (const [client] of interests.current) {
        if (!model.machines.some(machine => machine.client === client && machine.state.status === 'connected')) {
          interests.current.delete(client);
          void client.request('setVisibleSessions', { sessionIds: [] }).catch(() => {});
        }
      }
      for (const machine of model.machines) {
        if (machine.state.status !== 'connected') continue;
        const sessionIds = (machine.snapshot?.sessions ?? []).filter(session => visibleIds.current.has(`${machine.peer.id}:${session.id}`)).map(session => session.id).sort();
        const ids = sessionIds.join(',');
        const previous = interests.current.get(machine.client);
        if (previous?.connectionId === machine.state.connectionId && previous.ids === ids) continue;
        const interest = { connectionId: machine.state.connectionId, ids };
        interests.current.set(machine.client, interest);
        void machine.client.request('setVisibleSessions', { sessionIds }).then(result => {
          if (!result.ok) throw new Error(result.error.message);
        }).catch(cause => {
          if (interests.current.get(machine.client) === interest) interests.current.delete(machine.client);
          model.report(cause);
        });
      }
    };
    syncVisibility.current();
  }, [model.machines, model.report]);
  useEffect(() => () => {
    for (const client of interests.current.keys()) void client.request('setVisibleSessions', { sessionIds: [] }).catch(() => {});
    interests.current.clear();
    syncVisibility.current = () => {};
  }, []);
  const onViewableItemsChanged = useCallback(({ viewableItems }: { viewableItems: { item: unknown; index?: number | null; isViewable: boolean; section?: unknown }[] }) => {
    visibleIds.current = new Set(viewableItems.flatMap(token => token.isViewable && typeof token.index === 'number'
      && token.item && typeof token.item === 'object' && 'id' in token.item && typeof token.item.id === 'string'
      && token.section && typeof token.section === 'object' && 'key' in token.section && typeof token.section.key === 'string'
      ? [`${token.section.key}:${token.item.id}`] : []));
    syncVisibility.current();
  }, []);
  const rows = useMemo(
    () =>
      model.machines.map((machine) => ({
        key: machine.peer.id,
        machine,
        data: sortVisibleSessions(
          (machine.snapshot?.sessions ?? []).filter((session) => `${session.title} ${session.cwd}`.toLowerCase().includes(search.toLowerCase())),
          machine.busyTabIds,
        ),
      })),
    [model.machines, search],
  );
  const act = useCallback((operation: () => Promise<unknown>) => {
    void operation().catch(model.report);
  }, [model.report]);
  const create = (machine: MachineView) => {
    // A failed submission retries the same session creation, including after folder creation.
    const requestId = uuid();
    setDirectoryPrompt({
      id: requestId,
      title: `New session on ${machine.peer.hostname}`,
      api: machine.client,
      submit: async (cwd, signal) => {
        const result = await machine.client.request('createSession', { cwd, requestId });
        if (signal.aborted) return;
        if (!result.ok) throw new Error(result.error.message);
        await model.refresh(machine.client);
        if (!signal.aborted) open({ peerId: machine.peer.id, sessionId: result.value.id });
      },
    });
  };
  const manage = useCallback((client: MachineView['client'], session: SessionDto) => {
    const run = async (operation: Promise<{ ok: boolean; error?: { message: string } }>) => {
      const result = await operation;
      if (!result.ok) throw new Error(result.error?.message);
      await model.refresh(client);
    };
    const settle = async () => {
      if (session.settledAt) {
        await run(client.request('unsettleSession', { sessionId: session.id }));
        return;
      }
      const result = await client.request('settleSession', {
        sessionId: session.id,
        confirmActive: false,
      });
      if (!result.ok && result.error.code === 'SETTLE_CONFIRM_REQUIRED') {
        Alert.alert(
          'Settle active session?',
          'This marks the session settled without stopping its running terminals.',
          [
            { text: 'Cancel', style: 'cancel' },
            {
              text: 'Settle',
              onPress: () =>
                act(() =>
                  run(
                    client.request('settleSession', { sessionId: session.id, confirmActive: true }),
                  ),
                ),
            },
          ],
        );
      } else {
        if (!result.ok) throw new Error(result.error.message);
        await model.refresh(client);
      }
    };
    setMenu({
      title: session.title,
      detail: session.cwd,
      actions: [
        {
          title: 'Rename',
          action: () =>
            prompt({
              title: 'Rename session',
              label: 'Name',
              initial: session.title,
              submit: (title) =>
                run(client.request('renameSession', { sessionId: session.id, title })),
            }),
        },
        { title: 'Environment', action: () => setEnvironment({ client, session }) },
        {
          title: session.pinnedAt ? 'Unpin' : 'Pin',
          action: () =>
            act(() =>
              run(
                client.request('setSessionPinned', {
                  sessionId: session.id,
                  pinned: !session.pinnedAt,
                }),
              ),
            ),
        },
        { title: session.settledAt ? 'Restore session' : 'Settle session', action: () => act(settle) },
        {
          title: 'Delete session...',
          danger: true,
          action: () =>
            Alert.alert(
              'Delete remote session?',
              'This terminates all terminals in this session on the host.',
              [
                { text: 'Cancel', style: 'cancel' },
                {
                  text: 'Delete',
                  style: 'destructive',
                  onPress: () =>
                    act(() => run(client.request('deleteSession', { sessionId: session.id }))),
                },
              ],
            ),
        },
      ],
    });
  }, [act, model.refresh, prompt]);
  return (
    <View testID="sessions-screen" style={styles.page}>
      {directoryPrompt && <DirectoryPrompt config={directoryPrompt} onClose={() => setDirectoryPrompt(null)} />}
      {menu && <ActionMenu menu={menu} close={() => setMenu(null)} />}
      {environment && (
        <EnvironmentEditor
          session={environment.session}
          close={() => setEnvironment(null)}
          save={async (env) => {
            const result = await environment.client.request('setSessionEnv', {
              sessionId: environment.session.id,
              env,
            });
            if (!result.ok) throw new Error(result.error.message);
            await model.refresh(environment.client);
          }}
        />
      )}
      <View style={[styles.header, { paddingHorizontal: 12, paddingVertical: 4 }]}>
        <Text style={[styles.title, styles.grow]}>Sessions</Text>
        <ConnectionStatus model={model} />
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Search sessions"
          testID="search-sessions"
          accessibilityState={{ expanded: searching }}
          onPress={() => { setSearching(value => !value); setSearch(''); }}
          style={compact.control}
        >
          <View style={{ width: 20, height: 20 }}>
            <View style={{ width: 14, height: 14, borderRadius: 7, borderWidth: 2, borderColor: searching ? colors.accent : colors.muted }} />
            <View style={{ position: 'absolute', width: 8, height: 2, top: 14, left: 12, backgroundColor: searching ? colors.accent : colors.muted, transform: [{ rotate: '45deg' }] }} />
          </View>
        </Pressable>
      </View>
      {searching && (
        <TextInput
          accessibilityLabel="Find a session"
          placeholder="Name or directory"
          placeholderTextColor={colors.placeholder}
          value={search}
          onChangeText={setSearch}
          autoFocus autoCorrect={false} autoCapitalize="none"
          style={[styles.input, { marginHorizontal: 12, marginVertical: 6 }]}
        />
      )}
      {model.loading ? (
        <View testID="loading-sessions" style={compact.center}><StatusIndicator loading label="Loading sessions" /></View>
      ) : (
        <SectionList
          testID="sessions-list"
          accessibilityLabel="Sessions list"
          sections={rows}
          keyExtractor={session => session.id}
          stickySectionHeadersEnabled={false}
          onViewableItemsChanged={onViewableItemsChanged}
          viewabilityConfig={viewabilityConfig}
          keyboardShouldPersistTaps="handled"
          contentContainerStyle={[compact.content, !rows.length && { flexGrow: 1 }]}
          initialNumToRender={20}
          refreshing={refreshing}
          onRefresh={() => {
            setRefreshing(true);
            void Promise.all(model.machines.filter(item => item.state.status === 'connected').map(item => model.refresh(item.client)))
              .catch(model.report).finally(() => setRefreshing(false));
          }}
          ListEmptyComponent={<View testID="sessions-empty" accessibilityLabel="No sessions" style={compact.center}><Button title="Open Settings" onPress={settings} /></View>}
          renderSectionHeader={({ section: { machine } }) => (
            <View style={{ paddingTop: 4, paddingBottom: 2 }}>
              <View style={styles.row}>
                <View style={styles.grow}>
                  <Text numberOfLines={1} style={styles.label}>{machine.peer.hostname}</Text>
                  <Text style={[styles.subtitle, { marginTop: 0, color: machine.state.status === 'connected' ? colors.green : colors.muted }]}>
                    {machine.state.status.replaceAll('-', ' ')}
                  </Text>
                </View>
                {machine.state.status === 'connected' ? (
                  <Pressable
                    accessibilityRole="button"
                    accessibilityLabel={`New session on ${machine.peer.hostname}`}
                    onPress={() => create(machine)}
                    style={({ pressed }) => [compact.control, { borderRadius: 22, borderWidth: 1, borderColor: colors.line, opacity: pressed ? 0.65 : 1 }]}
                  >
                    <Text style={{ color: colors.text, fontSize: 26, lineHeight: 30 }}>+</Text>
                  </Pressable>
                ) : machine.state.status === 'awaiting-approval'
                  ? <Button title="Cancel request" subtle onPress={() => act(() => model.disconnect(machine.peer.id))} />
                  : <Button title="Connect" subtle onPress={() => act(() => model.connect(machine.peer))} />}
              </View>
              {machine.state.status === 'connected' && !machine.snapshot && (
                <View style={{ padding: 8 }}><StatusIndicator loading label={`Loading sessions on ${machine.peer.hostname}`} /></View>
              )}
              {machine.state.status === 'awaiting-approval' && <Note>Approve this phone in Shellfox on {machine.peer.hostname}.</Note>}
              {machine.state.lastError && !isConnectionFailure(machine.state.lastError) && <Note error>{machine.state.lastError.message}</Note>}
            </View>
          )}
          renderItem={({ item: session, section: { machine } }) => {
            const indicator = sessionDot(session, machine.busyTabIds);
            return <SessionRow session={session} peerId={machine.peer.id} client={machine.client} kind={indicator.kind} label={indicator.title} loading={machine.state.status !== 'connected' || machine.snapshotConnectionId !== machine.state.connectionId} open={open} manage={manage} />;
          }}
        />
      )}
    </View>
  );
}

const SessionRow = memo(function SessionRow({ session, peerId, client, kind, label, loading, open, manage }: {
  session: SessionDto;
  peerId: string;
  client: MachineView['client'];
  kind: DotState['kind'];
  label: string;
  loading: boolean;
  open: (selection: OpenSession) => void;
  manage: (client: MachineView['client'], session: SessionDto) => void;
}) {
  const { colors, styles } = useUi();
  return (
    <View style={[styles.row, { gap: 0, marginBottom: 4, borderWidth: 1, borderColor: colors.line, borderRadius: 4, backgroundColor: colors.panel }]}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Open ${session.title}`}
        accessibilityHint={`${label}. Long press for session actions`}
        onPress={() => open({ peerId, sessionId: session.id })}
        onLongPress={() => manage(client, session)}
        style={({ pressed }) => [compact.session, { opacity: pressed ? 0.7 : 1 }]}
      >
        <StatusIndicator kind={kind} loading={loading} label={loading ? 'Loading session' : label} />
        <View style={{ flex: 1, minWidth: 0 }}>
          <Text numberOfLines={1} style={styles.label}>{session.pinnedAt ? '• ' : ''}{session.title}</Text>
          <Text numberOfLines={1} style={[styles.subtitle, { marginTop: 0 }]}>{session.cwd}</Text>
        </View>
      </Pressable>
      <Pressable accessibilityRole="button" accessibilityLabel={`Manage ${session.title}`} onPress={() => manage(client, session)} style={compact.control}>
        <Text style={{ color: colors.muted, fontSize: 22 }}>⋯</Text>
      </Pressable>
    </View>
  );
});
const viewabilityConfig = { itemVisiblePercentThreshold: 1 };
const compact = StyleSheet.create({
  control: { width: 44, height: 44, alignItems: 'center', justifyContent: 'center' },
  center: { flex: 1, alignItems: 'center', justifyContent: 'center' },
  content: { paddingHorizontal: 12, paddingBottom: 12 },
  session: { flex: 1, minWidth: 44, minHeight: 44, flexDirection: 'row', alignItems: 'center', gap: 8, paddingHorizontal: 8, paddingVertical: 4 },
});
