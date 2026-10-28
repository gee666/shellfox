import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Keyboard, KeyboardAvoidingView, Platform, StyleSheet, Text, View } from 'react-native';
import { Button } from '../ui';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { WebView, type WebViewMessageEvent } from 'react-native-webview';
import { TerminalBridge, type MobileTerminalApi, type SubscribeTerminal, type TerminalBridgeState } from './bridge';
import { allowTerminalNavigation, emptyModifiers, parsePageMessage, type TerminalKey, type TerminalModifiers } from './protocol';
import { TERMINAL_HTML } from './terminal-html';
import { TerminalKeys } from './TerminalKeys';
import { useTheme } from '../theme';
import { showTerminalKeyboard } from './native-keyboard';
import { SpecialKeyboard } from './SpecialKeyboard';
import { useKeyboardOverlap } from './useKeyboardOverlap';
import { useTerminalAttachment } from './useTerminalAttachment';
import { sameUploadIdentity, type TerminalUploadApi } from './upload';

export interface TerminalViewProps {
  api: MobileTerminalApi;
  uploadApi?: TerminalUploadApi;
  subscribeTerminal: SubscribeTerminal;
  tabId: string;
  fontSize: number;
  /** Change after a remote reconnect to retire the old delivery view. */
  connectionEpoch: string | number;
  onError?: (error: Error) => void;
  onState?: (state: TerminalBridgeState) => void;
}
const source = { html: TERMINAL_HTML, baseUrl: 'about:blank' };
export function TerminalView({
  api,
  uploadApi,
  subscribeTerminal,
  tabId,
  fontSize,
  connectionEpoch,
  onError,
  onState,
}: TerminalViewProps) {
  const { colors, palette } = useTheme();
  const web = useRef<WebView>(null);
  const container = useRef<React.ComponentRef<typeof View>>(null);
  const { keyboardInset, keyboardOffset, keyboardVisible, measureKeyboardInset } = useKeyboardOverlap(container);
  const bridge = useRef<TerminalBridge | null>(null);
  const { accent, background } = palette;
  const callbacks = useRef({ onError, onState, fontSize, accent, background });
  callbacks.current = { onError, onState, fontSize, accent, background };
  const [viewId, setViewId] = useState<string | null>(null);
  const [modifiers, setModifiers] = useState<TerminalModifiers>(emptyModifiers);
  const modifiersRef = useRef<TerminalModifiers>(emptyModifiers);
  const [keyboardMode, setKeyboardMode] = useState<'closed' | 'system' | 'special'>('closed');
  const keyboardModeRef = useRef(keyboardMode);
  const changeKeyboardMode = (mode: typeof keyboardMode) => {
    keyboardModeRef.current = mode;
    setKeyboardMode(mode);
  };
  const [selecting, setSelecting] = useState(false);
  const [selection, setSelection] = useState('');
  const [blocked, setBlocked] = useState(false);
  const [terminalState, setTerminalState] = useState<TerminalBridgeState>('attaching');
  const attachment = useTerminalAttachment({
    api: uploadApi,
    contextKey: `${tabId}/${connectionEpoch}/${viewId ?? ''}`,
    enabled: terminalState === 'attached',
    getContext: () => {
      const captured = bridge.current;
      const identity = captured?.getUploadIdentity();
      if (!captured || !identity) return null;
      return { identity, current: () => bridge.current === captured && sameUploadIdentity(captured.getUploadIdentity(), identity) };
    },
    insert: text => bridge.current?.paste(text),
  });
  const [taking, setTaking] = useState(false);
  const insets = useSafeAreaInsets();
  const report = useCallback((error: Error) => callbacks.current.onError?.(error), []);

  useEffect(() => {
    // A terminal tap can summon the IME without using the accessory toggle.
    if (keyboardVisible) {
      keyboardModeRef.current = 'system';
      setKeyboardMode('system');
    } else {
      setKeyboardMode(mode => {
        const next = mode === 'system' ? 'closed' : mode;
        keyboardModeRef.current = next;
        return next;
      });
    }
  }, [keyboardVisible]);
  useEffect(() => {
    if (!viewId) return;
    const current = new TerminalBridge({
      api,
      subscribeTerminal,
      tabId,
      viewId,
      fontSize: callbacks.current.fontSize,
      theme: { accentColor: callbacks.current.accent, backgroundColor: callbacks.current.background },
      send: (message) => web.current?.postMessage(JSON.stringify(message)),
      onError: report,
      onState: state => {
        setTerminalState(state);
        if (state !== 'attached') attachment.invalidate();
        setBlocked(state === 'blocked');
        if (state === 'blocked') {
          keyboardModeRef.current = 'closed'; setKeyboardMode('closed'); setSelecting(false); setSelection(''); Keyboard.dismiss();
        }
        callbacks.current.onState?.(state);
      },
      onSelection: setSelection,
      onFocus: () => {
        if (Platform.OS === 'android' && keyboardModeRef.current === 'system') void showTerminalKeyboard().catch(report);
      },
    });
    bridge.current = current;
    setSelection(''); setBlocked(false); setTaking(false);
    setSelecting(false);
    modifiersRef.current = emptyModifiers;
    setModifiers(emptyModifiers);
    current.start();
    return () => {
      bridge.current = null;
      attachment.invalidate();
      current.dispose();
    };
  }, [api, subscribeTerminal, tabId, connectionEpoch, viewId, report]);
  useEffect(() => bridge.current?.setFontSize(fontSize), [fontSize]);
  useEffect(() => bridge.current?.setTheme({ accentColor: accent, backgroundColor: background }), [accent, background]);

  const onMessage = useCallback(
    (event: WebViewMessageEvent) => {
      const message = parsePageMessage(event.nativeEvent.data);
      if (!message) {
        report(new Error('Invalid terminal display message.'));
        return;
      }
      if (message.type === 'ready') {
        setViewId(message.viewId);
        return;
      }
      bridge.current?.receive(message);
    },
    [report],
  );
  const toggleKeyboard = () => {
    if (keyboardMode === 'system' || keyboardVisible) {
      changeKeyboardMode('closed');
      bridge.current?.focus(false);
      Keyboard.dismiss();
    } else {
      changeKeyboardMode('system');
      web.current?.requestFocus();
      bridge.current?.focus(true);
    }
  };
  const toggleSpecialKeyboard = () => {
    changeKeyboardMode(keyboardMode === 'special' ? 'closed' : 'special');
    bridge.current?.focus(false);
    Keyboard.dismiss();
  };
  const toggleModifier = (modifier: keyof TerminalModifiers) => {
    const next = { ...modifiersRef.current, [modifier]: !modifiersRef.current[modifier] };
    modifiersRef.current = next;
    setModifiers(next);
    bridge.current?.setModifiers(next);
  };
  const sendKey = (key: TerminalKey) => bridge.current?.key({ key, modifiers: modifiersRef.current });
  const specialKeyboard = keyboardMode === 'special' && !keyboardVisible;
  return (
    <View ref={container} onLayout={() => measureKeyboardInset()} style={[styles.container, { backgroundColor: colors.bg }]}>
      <KeyboardAvoidingView style={[styles.container, { paddingBottom: keyboardInset }]}
        keyboardVerticalOffset={keyboardOffset} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
        <View style={styles.container}>
        <WebView
          ref={web}
          source={source}
          style={[styles.web, { backgroundColor: colors.bg, opacity: blocked ? 0 : 1 }]}
          accessible={!blocked}
          accessibilityElementsHidden={blocked}
          importantForAccessibility={blocked ? 'no-hide-descendants' : 'auto'}
          pointerEvents={blocked ? 'none' : 'auto'}
          onMessage={onMessage}
          // A narrow originWhitelist opens rejected URLs in the OS browser before onShouldStart runs.
          // Route every URL through our deny-by-default callback instead.
          originWhitelist={['*']}
          onShouldStartLoadWithRequest={(request) => allowTerminalNavigation(request.url)}
          onNavigationStateChange={(state) => {
            if (!allowTerminalNavigation(state.url)) web.current?.stopLoading();
          }}
          onOpenWindow={() => undefined}
          javaScriptCanOpenWindowsAutomatically={false}
          setSupportMultipleWindows={false}
          allowFileAccess={false}
          allowFileAccessFromFileURLs={false}
          allowUniversalAccessFromFileURLs={false}
          mixedContentMode="never"
          thirdPartyCookiesEnabled={false}
          sharedCookiesEnabled={false}
          domStorageEnabled={false}
          cacheEnabled={false}
          incognito
          javaScriptEnabled
          scrollEnabled={false}
          bounces={false}
          keyboardDisplayRequiresUserAction={false}
          hideKeyboardAccessoryView={true}
          automaticallyAdjustContentInsets={false}
          contentInsetAdjustmentBehavior="never"
          allowsLinkPreview={false}
          dataDetectorTypes={['none']}
          textInteractionEnabled
          onError={(event) => report(new Error(event.nativeEvent.description))}
          onContentProcessDidTerminate={() => {
            setViewId(null);
            web.current?.reload();
          }}
          onRenderProcessGone={() => {
            setViewId(null);
            web.current?.reload();
          }}
        />
        {blocked && <View style={[styles.blocked, { backgroundColor: colors.bg }]}>
          <Text style={{ color: colors.muted, textAlign: 'center' }}>Another client is controlling this terminal</Text>
          <Button title={taking ? 'Taking...' : 'Take it'} disabled={taking} onPress={() => {
            setTaking(true);
            void (async () => { try { await bridge.current?.takeControl(); } finally { setTaking(false); } })();
          }} />
        </View>}
        </View>
        {!blocked && attachment.panel}
        {!blocked && <TerminalKeys
          attach={attachment.button}
          bottomInset={keyboardVisible || specialKeyboard ? 0 : insets.bottom}
          control={modifiers.ctrl}
          selecting={selecting}
          selection={selection}
          onControl={() => toggleModifier('ctrl')}
          onSelect={() => {
            const next = !selecting;
            setSelecting(next);
            bridge.current?.select(next);
            if (next) {
              changeKeyboardMode('closed');
              bridge.current?.focus(false);
              Keyboard.dismiss();
            }
          }}
          onKey={sendKey}
          systemKeyboard={keyboardVisible || keyboardMode === 'system'}
          specialKeyboard={keyboardMode === 'special'}
          onKeyboard={toggleKeyboard}
          onSpecialKeyboard={toggleSpecialKeyboard}
          onPaste={(text) => bridge.current?.paste(text)}
        />}
        {!blocked && specialKeyboard && <SpecialKeyboard modifiers={modifiers} onModifier={toggleModifier}
          onKey={sendKey} bottomInset={insets.bottom} />}
      </KeyboardAvoidingView>
    </View>
  );
}
const styles = StyleSheet.create({
  container: { flex: 1 },
  web: { flex: 1 },
  blocked: { position: 'absolute', left: 0, right: 0, top: 0, bottom: 0, justifyContent: 'center', alignItems: 'center', gap: 12, padding: 20 },
});
