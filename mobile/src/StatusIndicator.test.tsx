// @vitest-environment jsdom
import React from 'react';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { derivePalette } from '../../src/renderer/theme';
import { StatusIndicator } from './StatusIndicator';
import { ThemeProvider } from './theme';

type Props = { children?: React.ReactNode; style?: unknown; accessibilityLabel?: string; accessibilityRole?: string };
const animation = vi.hoisted(() => ({
  start: vi.fn(), stop: vi.fn(), timing: vi.fn(), setValue: vi.fn(),
  interpolate: vi.fn(({ outputRange }: { inputRange: number[]; outputRange: number[] }) => outputRange[0]),
}));
function view(props: Props) {
  return <div role={props.accessibilityRole === 'image' ? 'img' : props.accessibilityRole} aria-label={props.accessibilityLabel} data-style={JSON.stringify(props.style)}>{props.children}</div>;
}
vi.mock('react-native', () => ({
  View: (props: Props) => view(props),
  Animated: {
    Value: class { constructor(_initial: number) {} setValue = animation.setValue; interpolate = animation.interpolate; },
    View: (props: Props) => view(props),
    timing: (_value: unknown, config: unknown) => { animation.timing(config); return {}; },
    sequence: () => ({}), loop: () => ({ start: animation.start, stop: animation.stop }),
  },
  Easing: { ease: 'ease', linear: 'linear', inOut: (value: unknown) => value },
}));
beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe('mobile status indicator', () => {
  it.each([['shell', '#60a5fa'], ['running', '#4ade80'], ['busy', '#4ade80'], ['settled', '#96734e'], ['error', '#fc8397']] as const)('uses desktop %s color', (kind, color) => {
    const view = render(<StatusIndicator kind={kind} label="Status" />);
    const indicator = screen.getByRole('img', { name: 'Status' });
    expect(indicator.children).toHaveLength(1); expect(indicator.children[0].getAttribute('data-style')).toContain(color);
    expect(animation.start).toHaveBeenCalledTimes(kind === 'busy' ? 1 : 0);
    view.unmount(); expect(animation.stop).toHaveBeenCalledTimes(kind === 'busy' ? 1 : 0);
  });
  it.each([
    ['shell', 'Shell', '--dot-shell'],
    ['running', 'Agent idle', '--dot-running'],
    ['busy', 'Agent working', '--dot-running'],
    ['settled', 'Settled', '--dot-settled'],
    ['error', 'Error', '--dot-error'],
  ] as const)('updates %s to the desktop light-theme color without changing its label or animation', (kind, label, variable) => {
    const view = render(<ThemeProvider><StatusIndicator kind={kind} label={label} /></ThemeProvider>);
    view.rerender(<ThemeProvider accentColor="#ffff00" backgroundColor="#ffffff"><StatusIndicator kind={kind} label={label} /></ThemeProvider>);
    const palette = derivePalette('#ffff00', '#ffffff');
    const dot = screen.getByRole('img', { name: label }).children[0];
    expect(dot.getAttribute('data-style')).toContain(`"backgroundColor":"${palette.vars[variable]}"`);
    expect(palette.vars[variable]).not.toBe(palette.accent);
    expect(animation.start).toHaveBeenCalledTimes(kind === 'busy' ? 1 : 0);
    expect(animation.stop).not.toHaveBeenCalled();
  });
  it('uses desktop shell blue for all loading dots after switching to a light theme', () => {
    const view = render(<ThemeProvider><StatusIndicator loading label="Loading sessions" /></ThemeProvider>);
    view.rerender(<ThemeProvider accentColor="#ffff00" backgroundColor="#ffffff"><StatusIndicator loading label="Loading sessions" /></ThemeProvider>);
    const loading = screen.getByRole('progressbar', { name: 'Loading sessions' });
    const palette = derivePalette('#ffff00', '#ffffff');
    expect(loading.children).toHaveLength(3);
    for (const dot of loading.children) expect(dot.getAttribute('data-style')).toContain(`"backgroundColor":"${palette.vars['--dot-shell']}"`);
    expect(animation.start).toHaveBeenCalledOnce();
    expect(animation.stop).not.toHaveBeenCalled();
  });
  it('loops the loading dots forward from left to right without reversing', () => {
    const view = render(<StatusIndicator loading />);
    expect(animation.timing).toHaveBeenCalledExactlyOnceWith({ toValue: 1, duration: 900, easing: 'linear', useNativeDriver: true, isInteraction: false });
    expect(animation.interpolate.mock.calls.map(([config]) => config)).toEqual([
      { inputRange: [0, 1 / 3, 2 / 3, 1], outputRange: [1, 0.25, 0.25, 1] },
      { inputRange: [0, 1 / 3, 2 / 3, 1], outputRange: [0.25, 1, 0.25, 0.25] },
      { inputRange: [0, 1 / 3, 2 / 3, 1], outputRange: [0.25, 0.25, 1, 0.25] },
    ]);
    view.unmount(); expect(animation.stop).toHaveBeenCalledOnce();
  });
  it('uses a non-blocking 1.2-second native animation for working agents and stops when idle', () => {
    const view = render(<StatusIndicator kind="busy" label="Agent working" />);
    expect(animation.timing).toHaveBeenCalledTimes(2);
    expect(animation.timing).toHaveBeenCalledWith(expect.objectContaining({ duration: 600, useNativeDriver: true, isInteraction: false }));
    expect(animation.interpolate).toHaveBeenCalledWith({ inputRange: [0, 1], outputRange: [1, 0.45] });
    expect(animation.interpolate).toHaveBeenCalledWith({ inputRange: [0, 1], outputRange: [1, 0.85] });
    view.rerender(<StatusIndicator kind="running" label="Agent idle" />); expect(animation.stop).toHaveBeenCalledOnce();
  });
});
