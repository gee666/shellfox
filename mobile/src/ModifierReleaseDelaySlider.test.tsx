// @vitest-environment jsdom
import React from 'react';
import type { ViewProps } from 'react-native';
import { act, cleanup, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const native = vi.hoisted(() => ({ views: new Map<string, ViewProps>(), platform: { OS: 'android' } }));
vi.mock('@react-native-async-storage/async-storage', () => ({ default: {} }));
vi.mock('react-native', () => ({
  Platform: native.platform,
  View: (props: ViewProps) => {
    if (props.accessibilityLabel) native.views.set(props.accessibilityLabel, props);
    return <div role={props.accessibilityRole === 'adjustable' ? 'slider' : undefined}
      aria-label={props.accessibilityLabel} aria-valuemin={props.accessibilityValue?.min}
      aria-valuemax={props.accessibilityValue?.max} aria-valuenow={props.accessibilityValue?.now}
      aria-valuetext={props.accessibilityValue?.text} aria-disabled={props.accessibilityState?.disabled}>
      {props.children}
    </div>;
  },
  Text: ({ children }: { children?: React.ReactNode }) => <span>{children}</span>,
}));
vi.mock('./ui', () => ({ useUi: () => ({ colors: { line: '#333333', accent: '#ec4899' }, styles: {} }) }));
import { ModifierReleaseDelaySlider } from './ModifierReleaseDelaySlider';

const label = 'Sticky modifier release delay';
function props() {
  const result = native.views.get(label);
  if (!result) throw new Error('Slider not rendered');
  return result;
}
function invoke(name: keyof ViewProps, event: unknown = {}) {
  const handler = props()[name];
  if (typeof handler !== 'function') throw new Error(`Missing handler ${name}`);
  let result: unknown;
  act(() => { result = Reflect.apply(handler, undefined, [event]); });
  return result;
}
function pointer(name: keyof ViewProps, locationX: number) { invoke(name, { nativeEvent: { locationX } }); }
function adjust(actionName: string) { invoke('onAccessibilityAction', { nativeEvent: { actionName } }); }
function setup(value = 1000, disabled = false) {
  const onChange = vi.fn();
  const view = render(<ModifierReleaseDelaySlider value={value} disabled={disabled} onChange={onChange} />);
  invoke('onLayout', { nativeEvent: { layout: { width: 244 } } });
  return { onChange, rerender: (next: number, locked = false) => view.rerender(
    <ModifierReleaseDelaySlider value={next} disabled={locked} onChange={onChange} />,
  ) };
}
beforeEach(() => native.views.clear());
afterEach(cleanup);

describe.each(['android', 'ios'])('modifier release slider on %s', os => {
  beforeEach(() => { native.platform.OS = os; });

  it('exposes its range, value, units and adjustment actions', () => {
    setup();
    const slider = screen.getByRole('slider', { name: label });
    expect(slider).toHaveAttribute('aria-valuemin', '250');
    expect(slider).toHaveAttribute('aria-valuemax', '3000');
    expect(slider).toHaveAttribute('aria-valuenow', '1000');
    expect(slider).toHaveAttribute('aria-valuetext', '1000 milliseconds');
    expect(screen.getByText('1000 ms')).toBeInTheDocument();
    expect(screen.getByText('Shorter')).toBeInTheDocument();
    expect(screen.getByText('Longer')).toBeInTheDocument();
    expect(props().accessibilityActions).toEqual([{ name: 'increment', label: 'Longer' }, { name: 'decrement', label: 'Shorter' }]);
  });

  it('previews snapped drag values and commits only the final value on release', () => {
    const { onChange, rerender } = setup();
    expect(invoke('onStartShouldSetResponder')).toBe(true);
    pointer('onResponderGrant', 12);
    expect(screen.getByText('250 ms')).toBeInTheDocument();
    pointer('onResponderMove', 106);
    expect(screen.getByText('1500 ms')).toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
    expect(invoke('onResponderTerminationRequest')).toBe(false);
    pointer('onResponderRelease', 232);
    expect(onChange).toHaveBeenCalledExactlyOnceWith(3000);
    rerender(3000);
    expect(screen.getByText('3000 ms')).toBeInTheDocument();
  });

  it.each([-100, 500])('clamps touches outside the track at %i', location => {
    const { onChange } = setup();
    pointer('onResponderGrant', location);
    pointer('onResponderRelease', location);
    expect(onChange).toHaveBeenCalledExactlyOnceWith(location < 0 ? 250 : 3000);
  });

  it('discards a cancelled gesture and ignores moves or release without a drag', () => {
    const { onChange } = setup();
    pointer('onResponderGrant', 232);
    invoke('onResponderTerminate');
    expect(screen.getByText('1000 ms')).toBeInTheDocument();
    pointer('onResponderMove', 232);
    pointer('onResponderRelease', 232);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('adjusts by 250 ms for accessibility and does not save unchanged boundary values', () => {
    const { onChange, rerender } = setup();
    adjust('increment');
    expect(onChange).toHaveBeenLastCalledWith(1250);
    rerender(1250);
    adjust('decrement');
    expect(onChange).toHaveBeenLastCalledWith(1000);
    onChange.mockClear();
    rerender(250);
    adjust('decrement');
    rerender(3000);
    adjust('increment');
    adjust('activate');
    expect(onChange).not.toHaveBeenCalled();
  });

  it('does not write when a drag ends on the saved value', () => {
    const { onChange } = setup();
    pointer('onResponderGrant', 72);
    pointer('onResponderRelease', 72);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('disables touch and accessibility changes while busy', () => {
    const { onChange } = setup(1000, true);
    expect(screen.getByRole('slider')).toHaveAttribute('aria-disabled', 'true');
    expect(invoke('onStartShouldSetResponder')).toBe(false);
    expect(invoke('onMoveShouldSetResponder')).toBe(false);
    pointer('onResponderGrant', 232);
    pointer('onResponderMove', 232);
    pointer('onResponderRelease', 232);
    adjust('increment');
    adjust('decrement');
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByText('1000 ms')).toBeInTheDocument();
  });

  it('ignores gestures before usable layout and follows external value changes', () => {
    const onChange = vi.fn();
    const view = render(<ModifierReleaseDelaySlider value={1000} onChange={onChange} />);
    expect(invoke('onStartShouldSetResponder')).toBe(false);
    pointer('onResponderGrant', 232);
    pointer('onResponderRelease', 232);
    expect(onChange).not.toHaveBeenCalled();
    view.rerender(<ModifierReleaseDelaySlider value={750} onChange={onChange} />);
    expect(screen.getByText('750 ms')).toBeInTheDocument();
  });
});
