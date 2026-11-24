// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RustTerminalView } from './RustTerminalView';
import { RustHostClient } from '../rust-client/rust-host-client';
import { nativeTerminalForApi } from '../rust-client/native-terminal';
import { TestNative } from '../rust-client/test-native';
import { attachment, ids } from '../client/test-fixtures';

const probe = vi.hoisted(() => ({ copy: vi.fn(), invalidate: vi.fn(), dismiss: vi.fn(), visible: false }));
interface UiProps { children?: ReactNode; accessibilityLabel?: string; onPress?: () => void; disabled?: boolean; accessibilityState?: { selected?: boolean } }
interface NativeProbe {
  terminalHandle: string; readOnly?: boolean; focused?: boolean; selecting?: boolean; modifiers?: number;
  onInput?: (event: { nativeEvent: { terminalHandle: string } }) => void;
  onState?: (event: { nativeEvent: { terminalHandle: string; state: string; focusResult?: unknown } }) => void;
  onError?: (event: { nativeEvent: { terminalHandle: string; code: number } }) => void;
  onSelection?: (event: { nativeEvent: { terminalHandle: string; text: string } }) => void;
}
vi.mock('react-native', () => ({
  Platform: { OS: 'android' }, StyleSheet: { create: <T,>(styles: T) => styles },
  Keyboard: { dismiss: probe.dismiss }, Alert: { alert: vi.fn() },
  View: ({ children }: UiProps) => <div>{children}</div>,
  Text: ({ children }: UiProps) => <span>{children}</span>,
  ScrollView: ({ children }: UiProps) => <div>{children}</div>,
  KeyboardAvoidingView: ({ children }: UiProps) => <div>{children}</div>,
  Modal: ({ children, visible }: UiProps & { visible: boolean }) => visible ? <div>{children}</div> : null,
  TextInput: ({ value, onChangeText, accessibilityLabel }: { value: string; onChangeText(text: string): void; accessibilityLabel: string }) =>
    <textarea aria-label={accessibilityLabel} value={value} onChange={(event) => onChangeText(event.target.value)} />,
  Pressable: ({ children, onPress, accessibilityLabel, accessibilityState, disabled }: UiProps) =>
    <button aria-label={accessibilityLabel} aria-pressed={accessibilityState?.selected} disabled={disabled} onClick={onPress}>{children}</button>,
}));
vi.mock('react-native-safe-area-context', () => ({ useSafeAreaInsets: () => ({ bottom: 20 }) }));
vi.mock('./useKeyboardOverlap', () => ({ useKeyboardOverlap: () => ({
  keyboardInset: 0, keyboardOffset: 0, keyboardVisible: probe.visible, measureKeyboardInset: () => undefined,
}) }));
vi.mock('./useTerminalAttachment', () => ({ useTerminalAttachment: () => ({
  invalidate: probe.invalidate, panel: null, button: undefined,
}) }));
vi.mock('@react-native-clipboard/clipboard', () => ({ default: { setString: probe.copy, getString: async () => 'pasted 日本' } }));
vi.mock('../rust-client/specs/ShellfoxRustTerminalNativeComponent', () => ({ default: (props: NativeProbe) =>
  <div aria-label="Native terminal" data-modifiers={props.modifiers} data-readonly={String(props.readOnly)}
    data-focused={String(props.focused)} data-selecting={String(props.selecting)}>
    <button aria-label="Native input" onClick={() => props.onInput?.({ nativeEvent: { terminalHandle: props.terminalHandle } })} />
    {['bound', 'ready', 'recovering', 'closed', 'invalid'].map((state) => <button key={state}
      aria-label={`Native state ${state}`} onClick={() => props.onState?.({
        nativeEvent: { terminalHandle: props.terminalHandle, state },
      })} />)}
    <button aria-label="Focus feedback failure" onClick={() => props.onState?.({nativeEvent:{terminalHandle:props.terminalHandle,state:'focusResult',focusResult:focusPayload(false)}})} />
    <button aria-label="Focus feedback accepted" onClick={() => props.onState?.({nativeEvent:{terminalHandle:props.terminalHandle,state:'focusResult',focusResult:focusPayload(true)}})} />
    <button aria-label="Stale focus feedback" onClick={() => props.onState?.({nativeEvent:{terminalHandle:'999',state:'focusResult',focusResult:focusPayload(false)}})} />
    <button aria-label="Malformed focus feedback" onClick={() => props.onState?.({nativeEvent:{terminalHandle:props.terminalHandle,state:'focusResult',focusResult:{...focusPayload(false),payload:'DO_NOT_DISPLAY'}}})} />
    <button aria-label="Wrong-state focus feedback" onClick={() => props.onState?.({nativeEvent:{terminalHandle:props.terminalHandle,state:'ready',focusResult:focusPayload(true)}})} />
    <button aria-label="Native codegen default ready" onClick={() => props.onState?.({nativeEvent:{terminalHandle:props.terminalHandle,state:'ready',focusResult:{platform:'',source:'',attached:false,windowFocused:false,uiThread:false,inputAllowed:false,frameAvailable:false,focusRequest:'',focused:false,keyboardRequest:''}}})} />
    <button aria-label="Other terminal ready" onClick={() => props.onState?.({ nativeEvent: { terminalHandle: '999', state: 'ready' } })} />
    <button aria-label="Native failure" onClick={() => props.onError?.({ nativeEvent: { terminalHandle: props.terminalHandle, code: 5 } })} />
    <button aria-label="Native selection" onClick={() => props.onSelection?.({
      nativeEvent: { terminalHandle: props.terminalHandle, text: 'selected 日本' },
    })} />
  </div>,
}));

function focusPayload(accepted:boolean){return {platform:'android',source:'keyboard',attached:true,windowFocused:true,uiThread:true,inputAllowed:true,frameAvailable:true,focusRequest:accepted?'accepted':'rejected',focused:accepted,keyboardRequest:accepted?'accepted':'rejected'};}
const clients: RustHostClient[] = [];
async function tick(ms = 150) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); }
async function setup(readOnly = false, native = new TestNative(), ready = true) {
  const client = new RustHostClient('node_123', native, { createClientId: () => ids.client });
  clients.push(client); client.connect(); await tick();
  const driver = nativeTerminalForApi(client.terminalApi);
  if (!driver) throw new Error('Native driver is missing.');
  const onError = vi.fn();
  const rendered = render(<RustTerminalView driver={driver}
    tabId={ids.tab} fontSize={14} connectionEpoch="epoch1" modifierReleaseDelayMs={500} readOnly={readOnly} onError={onError} showKeyboardDiagnostics />);
  await tick();
  if (ready) fireEvent.click(screen.getByRole('button', { name: 'Native state ready' }));
  return { ...rendered, client, native, onError };
}
beforeEach(() => { vi.useFakeTimers(); vi.clearAllMocks(); probe.visible = false; });
afterEach(async () => {
  cleanup(); await tick(300);
  for (const client of clients.splice(0)) client.stop();
  await tick(); vi.useRealTimers();
});

describe('native terminal in the existing mobile controls', () => {
  it('mounts the bound native engine, configures existing theme and routes special keys through typed input', async () => {
    const { native } = await setup();
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly', 'false');
    expect(native.bindTerminal).toHaveBeenCalled();
    expect(native.configureTerminal).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Ctrl', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Esc', exact: true }));
    const escape = { physical: 120, modifiers: 2, consumedModifiers: 0, text: '', unshiftedCodepoint: null, composing: false };
    await tick(0);
    expect(native.terminalInput.mock.calls.map(([handle, json]) => [handle, JSON.parse(json)])).toEqual(
      ['press', 'release'].map(action => ['100', { type: 'key', event: { ...escape, action } }]));
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-modifiers', '2');
    fireEvent.click(screen.getByRole('button', { name: 'Native input' }));
    await tick(500);
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-modifiers', '0');
  });

  it('preserves keyboard toggles, native selection/copy and clipboard paste without terminal escapes', async () => {
    const { native } = await setup();
    fireEvent.click(screen.getByRole('button', { name: 'Keyboard', exact: true }));
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-focused', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Select', exact: true }));
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-focused', 'false');
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-selecting', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Native selection' }));
    fireEvent.click(screen.getByRole('button', { name: 'Copy', exact: true }));
    expect(probe.copy).toHaveBeenCalledWith('selected 日本');
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Paste', exact: true })); });
    fireEvent.click(screen.getByRole('button', { name: 'Send', exact: true }));
    expect(native.terminalInput).toHaveBeenCalledWith('100', JSON.stringify({ type: 'paste', text: 'pasted 日本' }));
  });

  it('renders current action failure without changing readiness, focus, input, attachment or native work',async()=>{
    const {native,onError}=await setup();const terminal=screen.getByLabelText('Native terminal');
    fireEvent.click(screen.getByRole('button',{name:'Keyboard',exact:true}));
    expect(screen.getByText('Keyboard SHOW. Snapshot since press: notObserved.')).toBeInTheDocument();
    const operations=native.operation.mock.calls.length,polls=native.poll.mock.calls.length,invalidated=probe.invalidate.mock.calls.length,dismissed=probe.dismiss.mock.calls.length;
    fireEvent.click(screen.getByRole('button',{name:'Focus feedback failure'}));
    expect(screen.getByText('Keyboard SHOW. Snapshot since press: observed; android/keyboard: focus=rejected, snapshot focused=no, request=rejected; attached=yes, window=yes, UI=yes, input=yes, frame=yes.')).toBeInTheDocument();
    expect(screen.getByLabelText('Native terminal')).toBe(terminal);expect(terminal).toHaveAttribute('data-readonly','false');expect(terminal).toHaveAttribute('data-focused','true');
    expect(native.operation).toHaveBeenCalledTimes(operations);expect(native.poll).toHaveBeenCalledTimes(polls);expect(native.terminalInput).not.toHaveBeenCalled();
    expect(probe.invalidate).toHaveBeenCalledTimes(invalidated);expect(probe.dismiss).toHaveBeenCalledTimes(dismissed);expect(onError).not.toHaveBeenCalled();
  });
  it('rejects stale and malformed feedback and cannot turn focus acceptance into parser readiness',async()=>{
    const {native,onError}=await setup(false,new TestNative(),false);const terminal=screen.getByLabelText('Native terminal');
    for(const name of ['Stale focus feedback','Malformed focus feedback','Wrong-state focus feedback','Focus feedback accepted'])fireEvent.click(screen.getByRole('button',{name}));
    expect(screen.queryByText(/Snapshot since press:/)).toBeNull();expect(screen.queryByText('DO_NOT_DISPLAY')).toBeNull();
    expect(terminal).toHaveAttribute('data-readonly','true');expect(terminal).toHaveAttribute('data-focused','false');
    expect(screen.queryByRole('button',{name:'Ctrl',exact:true})).toBeNull();expect(native.terminalInput).not.toHaveBeenCalled();expect(onError).not.toHaveBeenCalled();
  });
  it('shows separate SHOW/HIDE receipts and a later successful snapshot without claiming current focus',async()=>{
    const {native}=await setup();const terminal=screen.getByLabelText('Native terminal');
    fireEvent.click(screen.getByRole('button',{name:'Keyboard',exact:true}));
    expect(screen.getByText('Keyboard SHOW. Snapshot since press: notObserved.')).toBeInTheDocument();
    probe.visible = true; // the IME opened
    fireEvent.click(screen.getByRole('button',{name:'Focus feedback accepted'}));
    expect(screen.getByText(/Keyboard SHOW\. Snapshot since press: observed;.*snapshot focused=yes/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button',{name:'Keyboard',exact:true}));
    expect(screen.getByText(/^Keyboard HIDE\. Snapshot since press: notObserved\./)).toBeInTheDocument();
    expect(terminal).toHaveAttribute('data-focused','false');
    fireEvent.click(screen.getByRole('button',{name:'Focus feedback accepted'}));
    expect(screen.getByText(/Keyboard HIDE\. Snapshot since press: observed;.*snapshot focused=yes/)).toBeInTheDocument();
    expect(terminal).toHaveAttribute('data-focused','false');expect(terminal).toHaveAttribute('data-readonly','false');expect(native.terminalInput).not.toHaveBeenCalled();
  });
  it('does not carry a snapshot from before a press, accept stale data, or preserve it across recovery',async()=>{
    const {native,onError}=await setup();
    fireEvent.click(screen.getByRole('button',{name:'Focus feedback accepted'}));expect(screen.queryByText(/Snapshot since press:/)).toBeNull();
    fireEvent.click(screen.getByRole('button',{name:'Keyboard',exact:true}));
    for(const name of ['Stale focus feedback','Malformed focus feedback','Wrong-state focus feedback'])fireEvent.click(screen.getByRole('button',{name}));
    expect(screen.getByText('Keyboard SHOW. Snapshot since press: notObserved.')).toBeInTheDocument();expect(onError).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button',{name:'Focus feedback accepted'}));expect(screen.getByText(/Snapshot since press: observed/)).toBeInTheDocument();
    const operations=native.operation.mock.calls.length,polls=native.poll.mock.calls.length,binds=native.bindTerminal.mock.calls.length;
    fireEvent.click(screen.getByRole('button',{name:'Native state recovering'}));expect(screen.queryByText(/Snapshot since press:/)).toBeNull();
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly','true');
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-focused','true');
    fireEvent.click(screen.getByRole('button',{name:'Native state ready'}));expect(screen.queryByText(/Snapshot since press:/)).toBeNull();expect(native.terminalInput).not.toHaveBeenCalled();
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-focused','true');
    expect(native.operation).toHaveBeenCalledTimes(operations);expect(native.poll).toHaveBeenCalledTimes(polls);expect(native.bindTerminal).toHaveBeenCalledTimes(binds);
  });
  it('does not carry a prior view receipt or snapshot into a newly mounted view',async()=>{
    const old=await setup();
    fireEvent.click(screen.getByRole('button',{name:'Keyboard',exact:true}));fireEvent.click(screen.getByRole('button',{name:'Focus feedback accepted'}));
    expect(screen.getByText(/Snapshot since press: observed/)).toBeInTheDocument();
    old.unmount();await tick(300);await setup();
    expect(screen.queryByText(/Snapshot since press:/)).toBeNull();
    fireEvent.click(screen.getByRole('button',{name:'Focus feedback accepted'}));expect(screen.queryByText(/Snapshot since press:/)).toBeNull();
  });
  it('preserves normal iOS codegen readiness with its untouched empty nested value',async()=>{
    await setup(false,new TestNative(),false);fireEvent.click(screen.getByRole('button',{name:'Native codegen default ready'}));
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly','false');
    expect(screen.getByRole('button',{name:'Keyboard',exact:true})).toBeInTheDocument();
  });
  it('keeps input disabled until matching native readiness and disables it again during recovery', async () => {
    const { native } = await setup(false, new TestNative(), false);
    for (const name of ['Ctrl', 'Special keys', 'Paste']) {
      expect(screen.queryByRole('button', { name, exact: true })).toBeNull();
    }
    // A keyboard request may be made early: it waits for readiness.
    expect(screen.getByRole('button', { name: 'Keyboard', exact: true })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Native state bound' }));
    fireEvent.click(screen.getByRole('button', { name: 'Other terminal ready' }));
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly', 'true');
    expect(native.terminalInput).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Native state ready' }));
    fireEvent.click(screen.getByRole('button', { name: 'Special keys', exact: true }));
    expect(screen.getByRole('button', { name: 'F1', exact: true })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Ctrl', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Native state recovering' }));
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly', 'true');
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-modifiers', '0');
    expect(screen.queryByRole('button', { name: 'F1', exact: true })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Paste', exact: true })).toBeNull();
    expect(probe.invalidate).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Native state ready' }));
    fireEvent.click(screen.getByRole('button', { name: 'Esc', exact: true }));
    expect(native.terminalInput).toHaveBeenCalledTimes(1);
    expect(native.operation.mock.calls.every(([, json]) => !json.includes('writeTerminal'))).toBe(true);
  });

  it('fails closed on invalid readiness or native failure without replacing the native view', async () => {
    const { native, onError } = await setup();
    fireEvent.click(screen.getByRole('button', { name: 'Native state invalid' }));
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly', 'true');
    expect(onError).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Native state ready' }));
    fireEvent.click(screen.getByRole('button', { name: 'Native failure' }));
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly', 'true');
    expect(onError).toHaveBeenCalledTimes(2);
    expect(native.terminalInput).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Paste', exact: true })).toBeNull();
    expect(native.createTerminal).toHaveBeenCalledTimes(1);
  });

  it('retains selection/copy but hides all automation input controls', async () => {
    const { native } = await setup(true);
    expect(screen.getByLabelText('Native terminal')).toHaveAttribute('data-readonly', 'true');
    for (const name of ['Ctrl', 'Keyboard', 'Special keys', 'Paste']) expect(screen.queryByRole('button', { name, exact: true })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Select', exact: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Native selection' }));
    fireEvent.click(screen.getByRole('button', { name: 'Copy', exact: true }));
    expect(probe.copy).toHaveBeenCalledWith('selected 日本');
    expect(native.terminalInput).not.toHaveBeenCalled();
  });

  it('never takes ownership automatically and honors an explicit Take it action', async () => {
    const native = new TestNative();
    let owned = false;
    native.result = (method) => {
      if (method === 'takeTerminalControl') { owned = true; return { ok: true, value: { taken: true } }; }
      if (method === 'attachTerminal') {
        const { chunks: _chunks, lifetime: _lifetime, ...metadata } = attachment;
        return { ok: true, value: owned ? metadata : { control: 'blocked', tabId: ids.tab,
          sessionId: ids.session, generation: ids.generation, controlEpoch: 1 } };
      }
      return { ok: true, value: { detached: true } };
    };
    await setup(false, native);
    expect(native.operation.mock.calls.filter(([, json]) => json.includes('takeTerminalControl'))).toHaveLength(0);
    fireEvent.click(screen.getByRole('button', { name: 'Take it' })); await tick(300);
    expect(native.operation.mock.calls.filter(([, json]) => json.includes('takeTerminalControl'))).toHaveLength(1);
    expect(screen.queryByText('Another client is controlling this terminal')).toBeNull();
  });
});
