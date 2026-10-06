// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { ContextMenu, StatusDot, sessionDot, tabDot } from './components';
import { session } from './test-fixtures';
afterEach(cleanup);
describe('compact status and menus', () => {
  it('shows sticky session errors even when all open tabs are idle', () => {
    const s = session(1, {error:{code:'FOCUS_DENIED',message:'Focus denied',retryable:true}});
    expect(sessionDot(s, {})).toEqual({kind:'error',title:'Error: Focus denied'});
  });
  it('ignores closed-tab errors and pulses only running, busy agents', () => {
    const s = session();
    const t = s.tabs[0]!;
    expect(tabDot({...t,status:'waiting'},true).kind).toBe('shell');
    expect(tabDot({...t,status:'running'},false).kind).toBe('running');
    expect(tabDot({...t,status:'running'},true).kind).toBe('busy');
    expect(sessionDot({...s,tabs:[{...t,lifecycle:'closed',status:'error'}]},{}).kind).toBe('shell');
  });
  it.each([
    ['waiting', false, 'Shell'],
    ['running', false, 'Agent idle'],
    ['running', true, 'Agent working'],
  ] as const)('keeps %s dots accessible without hover popups', (status, busy, title) => {
    render(<StatusDot state={tabDot({ ...session().tabs[0]!, status }, busy)} />);
    expect(screen.getByLabelText(title).hasAttribute('title')).toBe(false);
  });
  it('dismisses context menus on outside pointerdown and Escape', () => {
    const close=vi.fn();
    render(<ContextMenu x={10} y={10} onClose={close}><button role="menuitem">Rename</button></ContextMenu>);
    fireEvent.pointerDown(screen.getByRole('menuitem'));
    expect(close).not.toHaveBeenCalled();
    fireEvent.pointerDown(document.body);
    expect(close).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document,{key:'Escape'});
    expect(close).toHaveBeenCalledTimes(2);
  });
});
