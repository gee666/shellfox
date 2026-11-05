import React, { useEffect, useRef, useState, type ComponentRef } from 'react';
import { KeyboardAvoidingView, Modal, Platform, ScrollView, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type { ManagerApi } from '../../src/shared/contracts';
import { DirectoryPathInput, type DirectoryPathInputHandle } from './DirectoryPathInput';
import { useKeyboardOverlap } from './terminal/useKeyboardOverlap';
import { Button, Note, useUi } from './ui';

type DirectoryApi = Required<Pick<ManagerApi, 'getHomeDirectory' | 'resolveDirectory' | 'createDirectory' | 'completeDirectory'>>;
export interface DirectoryPromptConfig {
  id: string;
  title: string;
  api: DirectoryApi;
  submit: (cwd: string, signal: AbortSignal) => Promise<void>;
}
export function DirectoryPrompt({ config, onClose }: { config: DirectoryPromptConfig; onClose: () => void }) {
  return <DirectoryPromptForm key={config.id} config={config} onClose={onClose} />;
}
function DirectoryPromptForm({ config, onClose }: { config: DirectoryPromptConfig; onClose: () => void }) {
  const { colors, styles } = useUi();
  const container = useRef<ComponentRef<typeof View>>(null);
  const { keyboardVisible, keyboardInset, keyboardOffset, measureKeyboardInset } = useKeyboardOverlap(container);
  const insets = useSafeAreaInsets();
  const lifetime = useRef(new AbortController());
  const pending = useRef(false);
  const pathInput = useRef<DirectoryPathInputHandle>(null);
  const [value, setValue] = useState('');
  const [homeDirectory, setHomeDirectory] = useState<string>();
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [missing, setMissing] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const loadHome = async () => {
    if (pending.current) return;
    const signal = lifetime.current.signal;
    pending.current = true;
    setLoading(true);
    setError(null);
    try {
      const home = await config.api.getHomeDirectory();
      if (signal.aborted) return;
      if (!home.ok) throw new Error(home.error.message);
      setHomeDirectory(home.value.cwd);
      setValue(home.value.cwd);
    } catch (cause) {
      if (!signal.aborted) setError(cause instanceof Error ? cause.message : 'Could not load the home directory.');
    } finally {
      if (!signal.aborted) { pending.current = false; setLoading(false); }
    }
  };
  useEffect(() => {
    const controller = new AbortController();
    lifetime.current = controller;
    pending.current = false;
    void loadHome();
    return () => { controller.abort(); };
  }, []);
  const close = () => {
    if (pending.current && !loading) return;
    lifetime.current.abort();
    onClose();
  };
  const requestClose = () => {
    if (!pathInput.current?.dismissSuggestions()) close();
  };
  const change = (draft: string) => { setError(null); setValue(draft); };
  const submit = async (create: boolean) => {
    if (pending.current || homeDirectory === undefined && !value.trim()) return;
    pending.current = true;
    const signal = lifetime.current.signal;
    pathInput.current?.dismissSuggestions();
    setBusy(true);
    setError(null);
    const path = create ? missing : value;
    try {
      if (path === null) return;
      const resolved = await (create ? config.api.createDirectory({ path }) : config.api.resolveDirectory({ path }));
      if (signal.aborted) return;
      if (!resolved.ok) {
        if (!create && resolved.error.code === 'NOT_FOUND') { setMissing(path); return; }
        throw new Error(resolved.error.message);
      }
      await config.submit(resolved.value.cwd, signal);
      if (!signal.aborted) { lifetime.current.abort(); onClose(); }
    } catch (cause) {
      if (!signal.aborted) setError(cause instanceof Error ? cause.message : 'Could not create the session.');
    } finally {
      if (!signal.aborted) { pending.current = false; setBusy(false); }
    }
  };
  return (
    <Modal transparent animationType="fade" visible onRequestClose={requestClose}>
      <View ref={container} onLayout={() => measureKeyboardInset()}
        style={{ flex: 1, backgroundColor: colors.backdrop }}>
        <KeyboardAvoidingView
          behavior={Platform.OS === 'ios' ? 'padding' : undefined}
          keyboardVerticalOffset={keyboardOffset}
          style={{ flex: 1, paddingBottom: keyboardInset }}
        >
          <ScrollView keyboardShouldPersistTaps="handled" style={{ flex: 1 }}
            contentContainerStyle={{ flexGrow: 1, justifyContent: 'center', padding: 22,
              paddingTop: Math.max(22, insets.top), paddingBottom: keyboardVisible ? 22 : Math.max(22, insets.bottom) }}>
            <View accessibilityViewIsModal style={styles.card}>
              <Note>{config.title}</Note>
              {missing !== null ? (
                <>
                  <Note>Create this folder and any missing parents?</Note>
                  <Note>{missing}</Note>
                  {error && <Note error>{error}</Note>}
                  <View style={styles.row}>
                    <View style={styles.grow}>
                      <Button title="No" subtle disabled={busy} onPress={() => { setMissing(null); setError(null); }} />
                    </View>
                    <View style={styles.grow}>
                      <Button title={busy ? 'Creating...' : 'Yes'} disabled={busy} onPress={() => { void submit(true); }} />
                    </View>
                  </View>
                </>
              ) : (
                <>
                  {!loading && (
                    <DirectoryPathInput ref={pathInput} api={config.api} value={value} onChangeText={change}
                      homeDirectory={homeDirectory ?? '~'} editable={!busy} autoFocus
                      onClose={close} onSubmit={() => { void submit(false); }} />
                  )}
                  {error && <Note error>{error}</Note>}
                  <View style={styles.row}>
                    <View style={styles.grow}><Button title="Cancel" subtle disabled={busy} onPress={close} /></View>
                    <View style={styles.grow}>
                      <Button title={loading ? 'Loading...' : busy ? 'Creating...' : 'OK'} disabled={busy || loading || homeDirectory === undefined && !value.trim()}
                        onPress={() => { void submit(false); }} />
                    </View>
                  </View>
                  {!loading && !value && error && <Button title="Retry" onPress={() => { void loadHome(); }} />}
                </>
              )}
            </View>
          </ScrollView>
        </KeyboardAvoidingView>
      </View>
    </Modal>
  );
}
