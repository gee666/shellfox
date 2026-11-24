import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Keyboard, KeyboardAvoidingView, Platform, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import NativeTerminal from '../rust-client/specs/ShellfoxRustTerminalNativeComponent';
import { terminalConfiguration } from '../rust-client/configure';
import { modifierBits } from '../rust-client/key-input';
import { nativeTerminalStateSchema } from '../rust-client/native-events';
import type { NativeLayoutPresentation, NativeViewDriver, NativeViewSession } from '../rust-client/native-terminal';
import { useTheme } from '../theme';
import { Button } from '../ui';
import type { TerminalViewProps } from './terminal-view-props';
import type { TerminalViewState } from './terminal-view-props';
import { terminalFontSize, utf8Size, type TerminalKey } from './protocol';
import { TerminalKeys } from './TerminalKeys';
import { SpecialKeyboard } from './SpecialKeyboard';
import { useKeyboardOverlap } from './useKeyboardOverlap';
import { useStickyModifiers } from './useStickyModifiers';
import { useTerminalAttachment } from './useTerminalAttachment';
import {useOwnedKeyboardMode} from './useOwnedKeyboardMode';
import { sameUploadIdentity } from './upload';
import { TerminalOperationError } from './errors';
import { nativeFileLink } from './native-file-link';
import {focusFeedbackSchema,focusActionNotice,isFocusFeedback,type FocusActionObservation} from './focus-feedback';
import {retirementNotice} from './keyboard-retirement';
import { appliesWantedKeyboard, closesForIme, keyBarState, keyboardTap, specialTap, surfaceFocused, wantsKeyboardAfterTap } from './key-bar-state';

export function RustTerminalView({ driver, tabId, uploadApi, readOnly = false, fontSize,
  modifierReleaseDelayMs = 1000, connectionEpoch, takeoverIntent, onTakeoverIntentConsumed,
  onError, onState, onFileOpen, showKeyboardDiagnostics = false,
}: Omit<TerminalViewProps, 'api'> & { driver: NativeViewDriver; showKeyboardDiagnostics?: boolean }) {
  const { colors, palette } = useTheme();
  const insets = useSafeAreaInsets();
  const container = useRef<React.ComponentRef<typeof View>>(null);
  const { keyboardInset, keyboardOffset, keyboardVisible, measureKeyboardInset } = useKeyboardOverlap(container);
  const current = useRef<NativeViewSession | null>(null);
  const callbacks = useRef({ onError, onState, onTakeoverIntentConsumed, theme: palette.terminal });
  callbacks.current = { onError, onState, onTakeoverIntentConsumed, theme: palette.terminal };
  const [session, setSession] = useState<NativeViewSession | null>(null);
  const [state, setState] = useState<TerminalViewState>('attaching');
  const [nativeReady, setNativeReady] = useState(false);
  const [focusObservation,setFocusObservation]=useState<FocusActionObservation|null>(null);
  const {mode: keyboardMode, retirement: keyboardRetirement, actionIntent: keyboardActionIntent, choose: chooseKeyboardMode, clear: clearKeyboardMode,
    recovering: recoverKeyboardMode, ready: readyKeyboardMode, layoutPending: layoutKeyboardPending,
    layoutFinished: layoutKeyboardFinished} = useOwnedKeyboardMode(
      {view: session, current, driver, tabId, connectionEpoch, readOnly}, keyboardVisible);
  // The user's keyboard request for this terminal. It outlives reattaches and
  // reconnects until the IME appears (see wantsKeyboardAfterTap).
  // A press queued from another terminal's render cannot set it.
  const [keyboardWanted, setKeyboardWanted] = useState<{ driver: NativeViewDriver; tabId: string } | null>(null);
  const wantsKeyboard = keyboardWanted?.driver === driver && keyboardWanted.tabId === tabId;
  const wantKeyboard = (value: boolean) => setKeyboardWanted(value ? { driver, tabId } : null);
  useEffect(() => setKeyboardWanted(null), [driver, tabId, readOnly]);
  const [selecting, setSelecting] = useState(false);
  const [selection, setSelection] = useState('');
  const [taking, setTaking] = useState(false);
  const dispatchedIntent = useRef<string | undefined>(undefined);
  // A failed native view reopens its terminal once per connection; a second failure stays visible.
  const [reopen, setReopen] = useState(0);
  const reopened = useRef(false);
  const [reopenWanted, setReopenWanted] = useState(false);
  useEffect(() => {
    if (!reopenWanted) return;
    const timer = setTimeout(() => { setReopenWanted(false); setReopen(value => value + 1); }, 1000);
    return () => clearTimeout(timer);
  }, [reopenWanted]);
  useEffect(() => { setReopenWanted(false); reopened.current = false; }, [driver, tabId, readOnly, connectionEpoch]);
  const { modifiers, current: modifiersRef, toggle, activity, reset } = useStickyModifiers(modifierReleaseDelayMs, () => undefined);
  const report = useCallback((cause: unknown) => callbacks.current.onError?.(
    cause instanceof Error ? cause : new Error('Native terminal failed.')), []);
  const attachment = useTerminalAttachment({
    api: uploadApi, contextKey: `${tabId}/${connectionEpoch}/${session?.handle ?? ''}`,
    enabled: !readOnly && nativeReady && state === 'attached',
    getContext: () => {
      const captured = current.current;
      const identity = captured?.getIdentity();
      return captured && identity ? { identity, current: () => current.current === captured
        && sameUploadIdentity(captured.getIdentity(), identity) } : null;
    },
    insert: text => { void current.current?.input({ type: 'paste', text }).catch(report); },
  });
  const invalidateAttachment = attachment.invalidate;
  const changeState = useCallback((next: TerminalViewState, presentation?: NativeLayoutPresentation) => {
    const view = current.current;
    const retainKeyboard = view && presentation?.phase === 'pending' && layoutKeyboardPending(view, presentation);
    setState(next);
    if (next === 'blocked' || next === 'closed') setKeyboardWanted(null);
    if (next !== 'attached') {
      setFocusObservation(null);setNativeReady(false); view?.nativeState('recovering'); reset(); invalidateAttachment();
      if (!retainKeyboard) clearKeyboardMode(true,'bridge_non_attached',presentation?.kind==='layoutRetired'?presentation.outcome:undefined);
    } else if (view) layoutKeyboardFinished(view, presentation);
    if (next !== 'attached') { setSelecting(false); setSelection(''); }
    callbacks.current.onState?.(next);
  }, [reset, invalidateAttachment, clearKeyboardMode, layoutKeyboardPending, layoutKeyboardFinished]);

  useEffect(() => {
    let disposed = false;
    let opened: NativeViewSession | undefined;
    let unsubscribe: () => void = () => undefined;
    let errors: () => void = () => undefined;
    changeState('attaching'); setSession(null); setFocusObservation(null); setSelection(''); setSelecting(false); reset();
    void driver.open(tabId, readOnly, terminalConfiguration(callbacks.current.theme)).then(async (view) => {
      opened = view;
      if (disposed) { await view.close(); return; }
      current.current = view; setSession(view);
      unsubscribe = view.subscribe((next, presentation) => {
        if (!disposed && current.current === view) changeState(next, presentation);
      });
      errors = view.subscribeError(error => {
        if (current.current === view) clearKeyboardMode(true,'session_error',undefined,view);
        report(error);
      });
      changeState(view.getState());
    }).catch((error: unknown) => {
      if (!disposed) {
        changeState(error instanceof TerminalOperationError && !error.detail.retryable ? 'closed' : 'reconnecting');
        report(error);
      }
    });
    return () => {
      disposed = true; unsubscribe(); errors(); clearKeyboardMode(true,'view_effect_cleanup'); current.current = null; invalidateAttachment();
      if (opened) void opened.close().catch(report);
    };
  }, [driver, tabId, readOnly, connectionEpoch, reopen, changeState, reset, invalidateAttachment, report, clearKeyboardMode]);

  useEffect(() => {
    if (session) void session.configure(terminalConfiguration(palette.terminal)).catch(report);
  }, [session, palette.terminal, report]);
  useEffect(() => {
    if (!session || !takeoverIntent || dispatchedIntent.current === takeoverIntent) return;
    dispatchedIntent.current = takeoverIntent;
    void session.takeControl().catch(report).finally(() => callbacks.current.onTakeoverIntentConsumed?.(takeoverIntent));
  }, [session, takeoverIntent, report]);

  const blocked = state === 'blocked';
  const inputEnabled = !readOnly && nativeReady && state === 'attached' && session?.getIdentity() != null;
  const { keyboardActive, specialActive, specialRow: specialKeyboard } = keyBarState(keyboardMode, keyboardVisible);
  // The IME appearing (tap, hardware toggle or another app) closes the Fn row.
  const imeWasVisible = useRef(keyboardVisible);
  useEffect(() => {
    const appeared = closesForIme(keyboardMode, imeWasVisible.current, keyboardVisible);
    imeWasVisible.current = keyboardVisible;
    if (appeared) clearKeyboardMode(false, 'ime_shown');
  }, [keyboardMode, keyboardVisible, clearKeyboardMode]);
  useEffect(() => { if (keyboardVisible) setKeyboardWanted(null); }, [keyboardVisible]);
  // Also the second half of a reopen: the native view must see focused go false, then true.
  useEffect(() => {
    if (appliesWantedKeyboard(wantsKeyboard, keyboardMode, keyboardVisible, inputEnabled)) chooseKeyboardMode('system', 'show');
  });
  const actionNotice = keyboardActionIntent===null ? null
    : focusObservation?.intent===keyboardActionIntent ? focusActionNotice(focusObservation)+(keyboardRetirement?` ${retirementNotice(keyboardRetirement)}`:'')
    : keyboardRetirement ? retirementNotice(keyboardRetirement) : null;
  const sendKey = (key: TerminalKey) => {
    void current.current?.key(key, modifiersRef.current).catch(report);
    activity();
  };
  const paste = (text: string) => {
    void current.current?.input({ type: 'paste', text }).catch(report); activity();
  };
  return <View ref={container} onLayout={() => measureKeyboardInset()} style={[styles.fill, { backgroundColor: colors.bg }]}>
    <KeyboardAvoidingView style={[styles.fill, { paddingBottom: keyboardInset }]}
      keyboardVerticalOffset={keyboardOffset} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
      <View style={styles.fill}>
        {session && <NativeTerminal terminalHandle={session.handle} style={[styles.fill, { backgroundColor: colors.bg, opacity: blocked ? 0 : 1 }]}
          readOnly={!inputEnabled} focused={keyboardMode === 'system'} selecting={selecting}
          modifiers={modifierBits(modifiers)} fontSize={terminalFontSize(fontSize)}
          accessible={!blocked} accessibilityElementsHidden={blocked}
          importantForAccessibility={blocked ? 'no-hide-descendants' : 'auto'} pointerEvents={blocked ? 'none' : 'auto'}
          onState={(event) => {
            const value = event.nativeEvent;
            if (value.terminalHandle !== session.handle || current.current !== session) return;
            if(isFocusFeedback(value)){
              const feedback=focusFeedbackSchema.safeParse(value);
              // A tap on the terminal asks for the system keyboard, as the toggle does.
              if (feedback.success && surfaceFocused(feedback.data.focusResult) && keyboardMode !== 'system') {
                wantKeyboard(true); chooseKeyboardMode('system');
              }
              if (!showKeyboardDiagnostics) return;
              if(feedback.success)setFocusObservation(previous=>previous?{...previous,snapshot:feedback.data.focusResult}:null);
              return; // Action feedback is never parser readiness or an ACK.
            }
            const parsed = nativeTerminalStateSchema.safeParse({ terminalHandle: value.terminalHandle, state: value.state });
            const next = parsed.success ? parsed.data.state : 'recovering';
            const retainKeyboard = parsed.success && next === 'recovering' && recoverKeyboardMode(session);
            session.nativeState(next); setNativeReady(next === 'ready');
            if (next === 'ready') readyKeyboardMode(session);
            else {
              setFocusObservation(null); setSelection(''); reset(); invalidateAttachment();
              if (!retainKeyboard) clearKeyboardMode(true,parsed.success?'native_state_not_retained':'native_readiness_invalid',undefined,session);
            }
            if (!parsed.success) report(new Error('Invalid native terminal readiness.'));
          }}
          onInput={(event) => { if (inputEnabled && event.nativeEvent.terminalHandle === session.handle) activity(); }}
          onSelection={(event) => {
            const value = event.nativeEvent;
            if (blocked || value.terminalHandle !== session.handle || current.current !== session) return;
            if (utf8Size(value.text) > 65536) { report(new Error('Native selection exceeds limit.')); return; }
            setSelection(value.text);
          }}
          onFileLink={onFileOpen && !blocked ? ({ nativeEvent }) => {
            if (current.current !== session || !nativeReady || (state !== 'attached' && !(state === 'closed' && readOnly))) return;
            const target = nativeFileLink(nativeEvent, session.handle);
            if (!target) return;
            setKeyboardWanted(null); clearKeyboardMode(true, 'selection_close');
            onFileOpen(target);
          } : undefined}
          onError={(event) => {
            const { terminalHandle, code } = event.nativeEvent;
            if (terminalHandle !== session.handle || current.current !== session) return;
            // Native drops input it can't send; only a too-large paste (2) or a real failure arrives.
            if (code === 2) { report(new Error('Paste a smaller section of text.')); return; }
            setFocusObservation(null);session.nativeState('closed'); setNativeReady(false); reset(); invalidateAttachment();
            clearKeyboardMode(true,'native_error',undefined,session); report(new Error('Native terminal display failed.'));
            if (!reopened.current) { reopened.current = true; setReopenWanted(true); }
          }}
        />}
        {!blocked && showKeyboardDiagnostics && actionNotice && <View pointerEvents="none" style={[styles.focusNotice,{backgroundColor:colors.bg}]}><Text style={{color:colors.muted}}>{actionNotice}</Text></View>}
        {blocked && <View style={[styles.blocked, { backgroundColor: colors.bg }]}>
          <Text style={{ color: colors.muted, textAlign: 'center' }}>Another client is controlling this terminal</Text>
          {!readOnly && <Button title={taking ? 'Taking...' : 'Take it'} disabled={taking} onPress={() => {
            setTaking(true); void current.current?.takeControl().catch(report).finally(() => setTaking(false));
          }} />}
        </View>}
      </View>
      {!blocked && attachment.panel}
      {!blocked && <TerminalKeys readOnly={!inputEnabled} keyboardToggle={!readOnly && state !== 'closed'}
        attach={inputEnabled ? attachment.button : undefined}
        bottomInset={keyboardVisible || specialKeyboard ? 0 : insets.bottom}
        control={modifiers.ctrl} selecting={selecting} selection={selection}
        onControl={() => toggle('ctrl')} onSelect={() => {
          setSelecting((value) => !value); setSelection(''); setKeyboardWanted(null); clearKeyboardMode(true,'selection_close');
        }} onKey={sendKey} systemKeyboard={keyboardActive}
        specialKeyboard={specialActive} onKeyboard={() => {
          const tap = keyboardTap(keyboardMode, keyboardVisible);
          wantKeyboard(wantsKeyboardAfterTap(tap));
          if (tap === 'reopen') { chooseKeyboardMode('closed'); return; }
          const next = tap === 'hide' ? 'closed' : 'system';
          // Receipt of this JS branch only. A later snapshot is not causally
          // correlated without a token, and HIDE need not yield one.
          const intent=next==='system'?'show':'hide';
          if (chooseKeyboardMode(next, intent) && showKeyboardDiagnostics) setFocusObservation({ intent, snapshot: null });
        }} onSpecialKeyboard={() => {
          const next = specialTap(keyboardMode, keyboardVisible);
          setKeyboardWanted(null); chooseKeyboardMode(next); if (next === 'special') Keyboard.dismiss();
        }} onPaste={paste} />}
      {!blocked && inputEnabled && specialKeyboard && <SpecialKeyboard modifiers={modifiers} onModifier={toggle}
        onKey={sendKey} bottomInset={insets.bottom} />}
    </KeyboardAvoidingView>
  </View>;
}
const styles = StyleSheet.create({
  fill: { flex: 1 },
  focusNotice:{position:'absolute',left:8,right:8,bottom:8,padding:4},
  blocked: { position: 'absolute', left: 0, right: 0, top: 0, bottom: 0, justifyContent: 'center', alignItems: 'center', gap: 12, padding: 20 },
});
