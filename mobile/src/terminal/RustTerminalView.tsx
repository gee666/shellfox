import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Keyboard, KeyboardAvoidingView, Platform, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import NativeTerminal from '../rust-client/specs/ShellfoxRustTerminalNativeComponent';
import { terminalConfiguration } from '../rust-client/configure';
import { keyInput, modifierBits } from '../rust-client/key-input';
import { nativeTerminalStateSchema } from '../rust-client/native-events';
import type { NativeLayoutPresentation, NativeViewDriver, NativeViewSession } from '../rust-client/native-terminal';
import { useTheme } from '../theme';
import { Button } from '../ui';
import type { TerminalViewProps } from './TerminalView';
import type { TerminalBridgeState } from './bridge';
import { terminalFontSize, utf8Size, type TerminalKey } from './protocol';
import { TerminalKeys } from './TerminalKeys';
import { SpecialKeyboard } from './SpecialKeyboard';
import { useKeyboardOverlap } from './useKeyboardOverlap';
import { useStickyModifiers } from './useStickyModifiers';
import { useTerminalAttachment } from './useTerminalAttachment';
import {useOwnedKeyboardMode} from './useOwnedKeyboardMode';
import { sameUploadIdentity } from './upload';
import { TerminalOperationError } from './errors';
import {focusFeedbackSchema,focusActionNotice,isFocusFeedback,type FocusActionObservation} from './focus-feedback';
import {retirementNotice} from './keyboard-retirement';

/** Loaded only after explicit engine selection and native capability validation. */
export function RustTerminalView({ driver, tabId, uploadApi, readOnly = false, fontSize,
  modifierReleaseDelayMs = 1000, connectionEpoch, takeoverIntent, onTakeoverIntentConsumed,
  onError, onState,
}: Omit<TerminalViewProps, 'api' | 'subscribeTerminal'> & { driver: NativeViewDriver }) {
  const { colors, palette } = useTheme();
  const insets = useSafeAreaInsets();
  const container = useRef<React.ComponentRef<typeof View>>(null);
  const { keyboardInset, keyboardOffset, keyboardVisible, measureKeyboardInset } = useKeyboardOverlap(container);
  const current = useRef<NativeViewSession | null>(null);
  const callbacks = useRef({ onError, onState, onTakeoverIntentConsumed, theme: palette.terminal });
  callbacks.current = { onError, onState, onTakeoverIntentConsumed, theme: palette.terminal };
  const [session, setSession] = useState<NativeViewSession | null>(null);
  const [state, setState] = useState<TerminalBridgeState>('attaching');
  const [nativeReady, setNativeReady] = useState(false);
  const [focusObservation,setFocusObservation]=useState<FocusActionObservation|null>(null);
  const {mode: keyboardMode, retirement: keyboardRetirement, actionIntent: keyboardActionIntent, choose: chooseKeyboardMode, clear: clearKeyboardMode,
    recovering: recoverKeyboardMode, ready: readyKeyboardMode, layoutPending: layoutKeyboardPending,
    layoutFinished: layoutKeyboardFinished} = useOwnedKeyboardMode(
      {view: session, current, driver, tabId, connectionEpoch, readOnly}, keyboardVisible);
  const [selecting, setSelecting] = useState(false);
  const [selection, setSelection] = useState('');
  const [taking, setTaking] = useState(false);
  const dispatchedIntent = useRef<string | undefined>(undefined);
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
  const changeState = useCallback((next: TerminalBridgeState, presentation?: NativeLayoutPresentation) => {
    const view = current.current;
    const retainKeyboard = view && presentation?.phase === 'pending' && layoutKeyboardPending(view, presentation);
    setState(next);
    if (next !== 'attached') {
      setFocusObservation(null);setNativeReady(false); view?.nativeState('recovering'); reset(); invalidateAttachment();
      if (!retainKeyboard) clearKeyboardMode(true,'bridge_non_attached',presentation?.kind==='layoutRetired'?presentation.outcome:undefined);
    } else if (view) layoutKeyboardFinished(view, presentation);
    if (next === 'blocked') { setSelecting(false); setSelection(''); }
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
  }, [driver, tabId, readOnly, connectionEpoch, changeState, reset, invalidateAttachment, report, clearKeyboardMode]);

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
  const specialKeyboard = keyboardMode === 'special' && !keyboardVisible;
  const actionNotice = keyboardActionIntent===null ? null
    : focusObservation?.intent===keyboardActionIntent ? focusActionNotice(focusObservation)+(keyboardRetirement?` ${retirementNotice(keyboardRetirement)}`:'')
    : keyboardRetirement ? retirementNotice(keyboardRetirement) : null;
  const sendKey = (key: TerminalKey) => {
    void current.current?.input(keyInput(key, modifiersRef.current)).catch(report);
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
              if(feedback.success)setFocusObservation(previous=>previous?{...previous,snapshot:feedback.data.focusResult}:null);
              return; // Action feedback is never parser readiness or an ACK.
            }
            const parsed = nativeTerminalStateSchema.safeParse({ terminalHandle: value.terminalHandle, state: value.state });
            const next = parsed.success ? parsed.data.state : 'recovering';
            const retainKeyboard = parsed.success && next === 'recovering' && recoverKeyboardMode(session);
            session.nativeState(next); setNativeReady(next === 'ready');
            if (next === 'ready') readyKeyboardMode(session);
            else {
              setFocusObservation(null); reset(); invalidateAttachment();
              if (!retainKeyboard) clearKeyboardMode(true,parsed.success?'native_state_not_retained':'native_readiness_invalid',undefined,session);
            }
            if (!parsed.success) report(new Error('Invalid native terminal readiness.'));
          }}
          onInput={(event) => { if (inputEnabled && event.nativeEvent.terminalHandle === session.handle) activity(); }}
          onSelection={(event) => {
            const value = event.nativeEvent;
            if (value.terminalHandle !== session.handle) return;
            if (utf8Size(value.text) > 65536) { report(new Error('Native selection exceeds limit.')); return; }
            setSelection(value.text);
          }}
          onError={(event) => {
            if (event.nativeEvent.terminalHandle === session.handle && current.current === session) {
              setFocusObservation(null);session.nativeState('closed'); setNativeReady(false); reset(); invalidateAttachment();
              clearKeyboardMode(true,'native_error',undefined,session); report(new Error('Native terminal display failed.'));
            }
          }}
        />}
        {!blocked && actionNotice && <View pointerEvents="none" style={[styles.focusNotice,{backgroundColor:colors.bg}]}><Text style={{color:colors.muted}}>{actionNotice}</Text></View>}
        {blocked && <View style={[styles.blocked, { backgroundColor: colors.bg }]}>
          <Text style={{ color: colors.muted, textAlign: 'center' }}>Another client is controlling this terminal</Text>
          {!readOnly && <Button title={taking ? 'Taking...' : 'Take it'} disabled={taking} onPress={() => {
            setTaking(true); void current.current?.takeControl().catch(report).finally(() => setTaking(false));
          }} />}
        </View>}
      </View>
      {!blocked && attachment.panel}
      {!blocked && <TerminalKeys readOnly={!inputEnabled} attach={inputEnabled ? attachment.button : undefined}
        bottomInset={keyboardVisible || specialKeyboard ? 0 : insets.bottom}
        control={modifiers.ctrl} selecting={selecting} selection={selection}
        onControl={() => toggle('ctrl')} onSelect={() => {
          setSelecting((value) => !value); clearKeyboardMode(true,'selection_close');
        }} onKey={sendKey} systemKeyboard={keyboardVisible || keyboardMode === 'system'}
        specialKeyboard={keyboardMode === 'special'} onKeyboard={() => {
          const next = keyboardMode === 'system' || keyboardVisible ? 'closed' : 'system';
          // Receipt of this JS branch only. A later snapshot is not causally
          // correlated without a token, and HIDE need not yield one.
          const intent=next==='system'?'show':'hide';
          if(chooseKeyboardMode(next,intent))setFocusObservation({intent,snapshot:null});
        }} onSpecialKeyboard={() => {
          const next = keyboardMode === 'special' ? 'closed' : 'special';
          chooseKeyboardMode(next); if (next === 'special') Keyboard.dismiss();
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
