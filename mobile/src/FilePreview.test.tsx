// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { FilePreview } from './FilePreview';
import { WEBVIEW_LOAD_TIMEOUT_MS } from './useRecoveringWebView';

interface NativeWebProps {
  accessibilityValue: { text: string };
  onLoad: () => void;
  onError: (event: { nativeEvent: { description: string } }) => void;
  onContentProcessDidTerminate: () => void;
}
let webProps: NativeWebProps | null = null;
let mounts = 0;
vi.mock('react-native', () => ({ View: ({ children }: { children?: ReactNode }) => <div>{children}</div> }));
vi.mock('./ui', () => ({
  useUi: () => ({ colors: { bg: '#000', text: '#fff' }, styles: {} }),
  Note: ({ children }: { children: ReactNode }) => <span role="status">{children}</span>,
  Button: ({ title, onPress }: { title: string; onPress: () => void }) => <button onClick={onPress}>{title}</button>,
}));
vi.mock('react-native-webview', () => ({ WebView: (props: NativeWebProps) => {
  webProps = props;
  React.useEffect(() => { mounts += 1; }, []);
  return <div />;
} }));
beforeEach(() => { vi.useFakeTimers(); mounts = 0; });
afterEach(() => { cleanup(); webProps = null; vi.useRealTimers(); });
const web = () => {
  if (!webProps) throw new Error('Missing WebView');
  return webProps;
};

it('reports natively when the preview document has loaded, and again for new content', () => {
  const view = render(<FilePreview text="first" kind="code" onError={() => undefined} />);
  expect(web().accessibilityValue).toEqual({ text: 'Loading' });
  act(() => web().onLoad());
  expect(web().accessibilityValue).toEqual({ text: 'Loaded' });
  act(() => vi.advanceTimersByTime(WEBVIEW_LOAD_TIMEOUT_MS * 2));
  expect(mounts).toBe(1);
  view.rerender(<FilePreview text="second" kind="code" onError={() => undefined} />);
  expect(web().accessibilityValue).toEqual({ text: 'Loading' });
});

it('replaces a WebView that never loads, then offers a retry instead of a blank page', () => {
  const onError = vi.fn();
  render(<FilePreview text="stuck" kind="code" onError={onError} />);
  act(() => vi.advanceTimersByTime(WEBVIEW_LOAD_TIMEOUT_MS));
  expect(mounts).toBe(2);
  act(() => vi.advanceTimersByTime(WEBVIEW_LOAD_TIMEOUT_MS));
  expect(mounts).toBe(3);
  expect(onError).not.toHaveBeenCalled();
  act(() => vi.advanceTimersByTime(WEBVIEW_LOAD_TIMEOUT_MS));
  expect(onError).toHaveBeenCalledWith('The preview did not load.');
  expect(screen.getByRole('status').textContent).toBe('Preview did not load.');
  fireEvent.click(screen.getByText('Retry'));
  expect(mounts).toBe(4);
  act(() => web().onLoad());
  expect(web().accessibilityValue).toEqual({ text: 'Loaded' });
});

it('reloads after the content process dies and keeps later navigation errors on the page', () => {
  const onError = vi.fn();
  render(<FilePreview text="page" kind="html" onError={onError} />);
  act(() => web().onLoad());
  act(() => web().onContentProcessDidTerminate());
  expect(mounts).toBe(2);
  expect(web().accessibilityValue).toEqual({ text: 'Loading' });
  act(() => web().onLoad());
  act(() => web().onError({ nativeEvent: { description: 'blocked' } }));
  expect(mounts).toBe(2);
  expect(onError).toHaveBeenCalledWith('blocked');
});
