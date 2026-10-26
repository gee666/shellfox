import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Keyboard, KeyboardAvoidingView, Platform, StyleSheet, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { WebView, type WebViewMessageEvent } from 'react-native-webview';
import { TerminalBridge, type MobileTerminalApi, type SubscribeTerminal } from './bridge';
import { allowTerminalNavigation, controlInput, parsePageMessage } from './protocol';
import { TERMINAL_HTML } from './terminal-html';
import { TerminalKeys } from './TerminalKeys';
import { useTheme } from '../theme';
import { showTerminalKeyboard } from './native-keyboard';

export interface TerminalViewProps {
  api: MobileTerminalApi;
  subscribeTerminal: SubscribeTerminal;
  tabId: string;
  fontSize: number;
  /** Change after a remote reconnect to retire the old delivery view. */
  connectionEpoch: string | number;
  onError?: (error: Error) => void;
}
const source = { html: TERMINAL_HTML, baseUrl: 'about:blank' };
export function TerminalView({
  api,
  subscribeTerminal,
  tabId,
  fontSize,
  connectionEpoch,
  onError,
}: TerminalViewProps) {
  const { colors, palette } = useTheme();
  const web = useRef<WebView>(null);
  const container = useRef<React.ComponentRef<typeof View>>(null);
  const [keyboardInset, setKeyboardInset] = useState(0);
  const [keyboardOffset, setKeyboardOffset] = useState(0);
  const measureKeyboardInset = useCallback((top = Keyboard.metrics()?.screenY) => {
    container.current?.measureInWindow((_x, y, _width, height) => {
      if (Platform.OS === 'ios') { setKeyboardOffset(y); return; }
      // Android 15 edge-to-edge keeps the window full-height even with adjustResize.
      // Measure overlap so already-resized parents never lose height twice.
      setKeyboardInset(top === undefined || !Keyboard.isVisible() ? 0 : Math.max(0, y + height - top));
    });
  }, []);
  const bridge = useRef<TerminalBridge | null>(null);
  const { accent, background } = palette;
  const callbacks = useRef({ onError, fontSize, accent, background });
  callbacks.current = { onError, fontSize, accent, background };
  const [viewId, setViewId] = useState<string | null>(null);
  const [control, setControl] = useState(false);
  const controlRef = useRef(false);
  const [selecting, setSelecting] = useState(false);
  const [selection, setSelection] = useState('');
  const [keyboardVisible, setKeyboardVisible] = useState(Keyboard.isVisible());
  const insets = useSafeAreaInsets();
  const report = useCallback((error: Error) => callbacks.current.onError?.(error), []);

  useEffect(() => {
    measureKeyboardInset();
    const show = Keyboard.addListener('keyboardDidShow', event => {
      setKeyboardVisible(true);
      measureKeyboardInset(event.endCoordinates.screenY);
    });
    const hide = Keyboard.addListener('keyboardDidHide', () => {
      setKeyboardVisible(false);
      setKeyboardInset(0);
    });
    return () => {
      show.remove();
      hide.remove();
    };
  }, [measureKeyboardInset]);
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
      onSelection: setSelection,
      onFocus: () => {
        if (Platform.OS === 'android') void showTerminalKeyboard().catch(report);
      },
    });
    bridge.current = current;
    setSelection('');
    setSelecting(false);
    controlRef.current = false;
    setControl(false);
    current.start();
    return () => {
      bridge.current = null;
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
      if (message.type === 'input')
        bridge.current?.receive({ ...message, data: controlInput(message.data, controlRef.current) });
      else bridge.current?.receive(message);
    },
    [report],
  );
  const toggleKeyboard = () => {
    if (keyboardVisible) {
      bridge.current?.focus(false);
      Keyboard.dismiss();
    } else {
      web.current?.requestFocus();
      bridge.current?.focus(true);
    }
  };
  return (
    <View ref={container} onLayout={() => measureKeyboardInset()} style={[styles.container, { backgroundColor: colors.bg }]}>
      <KeyboardAvoidingView style={[styles.container, { paddingBottom: keyboardInset }]}
        keyboardVerticalOffset={keyboardOffset} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
        <WebView
          ref={web}
          source={source}
          style={[styles.web, { backgroundColor: colors.bg }]}
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
        <TerminalKeys
          bottomInset={keyboardVisible ? 0 : insets.bottom}
          control={control}
          selecting={selecting}
          selection={selection}
          onControl={() => {
            controlRef.current = !controlRef.current;
            setControl(controlRef.current);
          }}
          onSelect={() => {
            const next = !selecting;
            setSelecting(next);
            bridge.current?.select(next);
            if (next) Keyboard.dismiss();
          }}
          onKey={(data) => bridge.current?.input(controlInput(data, controlRef.current))}
          onKeyboard={toggleKeyboard}
          onPaste={(text) => bridge.current?.paste(text)}
        />
      </KeyboardAvoidingView>
    </View>
  );
}
const styles = StyleSheet.create({
  container: { flex: 1 },
  web: { flex: 1 },
});
