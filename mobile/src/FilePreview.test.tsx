// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, render } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { FilePreview } from './FilePreview';

interface NativeWebProps { accessibilityValue: { text: string }; onLoadEnd: () => void }
let webProps: NativeWebProps | null = null;
vi.mock('react-native', () => ({ View: ({ children }: { children?: ReactNode }) => <div>{children}</div> }));
vi.mock('./ui', () => ({ useUi: () => ({ colors: { bg: '#000', text: '#fff' }, styles: {} }) }));
vi.mock('react-native-webview', () => ({ WebView: React.forwardRef<unknown, NativeWebProps>((props, _ref) => {
  webProps = props;
  return <div />;
}) }));
afterEach(() => { cleanup(); webProps = null; });
const web = () => {
  if (!webProps) throw new Error('Missing WebView');
  return webProps;
};

it('reports natively when the preview document has loaded, and again for new content', () => {
  const view = render(<FilePreview text="first" kind="code" onError={() => undefined} />);
  expect(web().accessibilityValue).toEqual({ text: 'Loading' });
  act(() => web().onLoadEnd());
  expect(web().accessibilityValue).toEqual({ text: 'Loaded' });
  view.rerender(<FilePreview text="second" kind="code" onError={() => undefined} />);
  expect(web().accessibilityValue).toEqual({ text: 'Loading' });
});
