import React, { useMemo, useState } from 'react';
import { Alert, FlatList, Pressable, Text, View } from 'react-native';
import { v4 as uuid } from 'uuid';
import type { SessionDto } from '../../src/shared/contracts';
import type { MachineView, ShellfoxModel } from './useShellfox';
import { Button, Card, Field, Note, colors, styles } from './ui';
import type { PromptConfig } from './Prompt';
import { ActionMenu, type MenuConfig } from './ActionMenu';
import { EnvironmentEditor } from './EnvironmentEditor';

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
  const [search, setSearch] = useState('');
  const [menu, setMenu] = useState<MenuConfig | null>(null);
  const [environment, setEnvironment] = useState<{ machine: MachineView; session: SessionDto } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const rows = useMemo(
    () =>
      model.machines.map((machine) => ({
        machine,
        sessions: (machine.snapshot?.sessions ?? [])
          .filter((session) => `${session.title} ${session.cwd}`.toLowerCase().includes(search.toLowerCase()))
          .sort(
            (a, b) => Number(!!b.pinnedAt) - Number(!!a.pinnedAt) || b.updatedAt.localeCompare(a.updatedAt),
          ),
      })),
    [model.machines, search],
  );
  const act = (operation: () => Promise<unknown>) => {
    void operation().catch(model.report);
  };
  const create = (machine: MachineView) => {
    act(async () => {
      const home = await machine.client.request('getHomeDirectory', {});
      if (!home.ok) throw new Error(home.error.message);
      // Retain one request ID across a failed submission so a retry cannot duplicate a session.
      const requestId = uuid();
      const submit = async (path: string, createDirectory: boolean) => {
        const resolved = await machine.client.request(
          createDirectory ? 'createDirectory' : 'resolveDirectory',
          { path },
        );
        if (!resolved.ok) throw new Error(resolved.error.message);
        const result = await machine.client.request('createSession', { cwd: resolved.value.cwd, requestId });
        if (!result.ok) throw new Error(result.error.message);
        await model.refresh(machine.client);
        open({ peerId: machine.peer.id, sessionId: result.value.id });
      };
      prompt({
        title: `New session on ${machine.peer.hostname}`,
        label: 'Host directory',
        initial: home.value.cwd,
        submitLabel: 'Create session',
        submit: (path) => submit(path, false),
        secondary: { title: 'Create folder and session', action: (path) => submit(path, true) },
      });
    });
  };
  const manage = (machine: MachineView, session: SessionDto) => {
    const run = async (operation: Promise<{ ok: boolean; error?: { message: string } }>) => {
      const result = await operation;
      if (!result.ok) throw new Error(result.error?.message);
      await model.refresh(machine.client);
    };
    const settle = async () => {
      if (session.settledAt) {
        await run(machine.client.request('unsettleSession', { sessionId: session.id }));
        return;
      }
      const result = await machine.client.request('settleSession', {
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
                    machine.client.request('settleSession', { sessionId: session.id, confirmActive: true }),
                  ),
                ),
            },
          ],
        );
      } else {
        if (!result.ok) throw new Error(result.error.message);
        await model.refresh(machine.client);
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
                run(machine.client.request('renameSession', { sessionId: session.id, title })),
            }),
        },
        { title: 'Environment', action: () => setEnvironment({ machine, session }) },
        {
          title: session.pinnedAt ? 'Unpin' : 'Pin',
          action: () =>
            act(() =>
              run(
                machine.client.request('setSessionPinned', {
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
                    act(() => run(machine.client.request('deleteSession', { sessionId: session.id }))),
                },
              ],
            ),
        },
      ],
    });
  };
  return (
    <View style={styles.page}>
      {menu && <ActionMenu menu={menu} close={() => setMenu(null)} />}
      {environment && (
        <EnvironmentEditor
          session={environment.session}
          close={() => setEnvironment(null)}
          save={async (env) => {
            const result = await environment.machine.client.request('setSessionEnv', {
              sessionId: environment.session.id,
              env,
            });
            if (!result.ok) throw new Error(result.error.message);
            await model.refresh(environment.machine.client);
          }}
        />
      )}
      <View style={styles.header}>
        <View>
          <Text style={styles.title}>Sessions</Text>
          <Text style={styles.subtitle}>Your machines. Your running shells.</Text>
        </View>
        <Button title="Machines" subtle onPress={settings} />
      </View>
      <FlatList
        data={rows}
        keyExtractor={(item) => item.machine.peer.id}
        keyboardShouldPersistTaps="handled"
        contentContainerStyle={styles.content}
        refreshing={refreshing}
        onRefresh={() => {
          setRefreshing(true);
          void Promise.all(
            model.machines
              .filter((item) => item.state.status === 'connected')
              .map((item) => model.refresh(item.client)),
          )
            .catch(model.report)
            .finally(() => setRefreshing(false));
        }}
        ListHeaderComponent={
          <Field
            label="Find a session"
            placeholder="Name or directory"
            value={search}
            onChangeText={setSearch}
            autoCorrect={false}
          />
        }
        ListEmptyComponent={
          <Card>
            <Text style={styles.label}>Connect to your first machine</Text>
            <Note>
              Sign in to Tailscale in Settings, then choose a running Shellfox host. Your sessions stay on
              that machine.
            </Note>
            <Button title="Open Settings" onPress={settings} />
          </Card>
        }
        renderItem={({ item: { machine, sessions } }) => (
          <View style={{ gap: 10 }}>
            <View style={styles.row}>
              <View style={styles.grow}>
                <Text style={styles.label}>{machine.peer.hostname}</Text>
                <Text
                  style={[
                    styles.subtitle,
                    { color: machine.state.status === 'connected' ? colors.green : colors.muted },
                  ]}
                >
                  {machine.state.status.replaceAll('-', ' ')}
                </Text>
              </View>
              {machine.state.status === 'connected' ? (
                <Button title="+ Session" subtle onPress={() => create(machine)} />
              ) : (
                <Button title="Connect" subtle onPress={() => act(() => model.connect(machine.peer))} />
              )}
            </View>
            {machine.state.status === 'awaiting-approval' && (
              <Note>
                Approve this phone in Shellfox on {machine.peer.hostname}. This page will connect
                automatically.
              </Note>
            )}
            {machine.state.lastError && machine.state.status !== 'awaiting-approval' && (
              <Note error>{machine.state.lastError.message}</Note>
            )}
            {sessions.map((session) => (
              <Pressable
                key={session.id}
                accessibilityRole="button"
                accessibilityLabel={`Open ${session.title}`}
                onPress={() => open({ peerId: machine.peer.id, sessionId: session.id })}
                onLongPress={() => manage(machine, session)}
                style={({ pressed }) => [styles.card, { opacity: pressed ? 0.7 : 1 }]}
              >
                <View style={styles.row}>
                  <Text numberOfLines={1} style={[styles.label, styles.grow]}>
                    {session.pinnedAt ? '• ' : ''}
                    {session.title}
                  </Text>
                  <Pressable
                    accessibilityRole="button"
                    accessibilityLabel={`Manage ${session.title}`}
                    hitSlop={12}
                    onPress={() => manage(machine, session)}
                  >
                    <Text style={{ color: colors.muted, fontSize: 22 }}>⋯</Text>
                  </Pressable>
                </View>
                <Text numberOfLines={1} style={styles.subtitle}>
                  {session.cwd}
                </Text>
                <Note>
                  {session.tabs.filter((tab) => tab.lifecycle !== 'closed').length} terminals ·{' '}
                  {session.status}
                  {session.counts.agents ? ` · ${session.counts.agents} agents` : ''}
                </Note>
              </Pressable>
            ))}
            {machine.state.status === 'connected' && !sessions.length && (
              <Note>
                {search ? 'No matching sessions.' : 'No sessions yet. Create one in a host directory.'}
              </Note>
            )}
          </View>
        )}
      />
    </View>
  );
}
