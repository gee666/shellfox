import React from 'react';
import { View } from 'react-native';
import { nativeTerminalForApi } from '../rust-client/native-terminal';
import { WebViewTerminalView } from './WebViewTerminalView';
import type { TerminalViewProps } from './terminal-view-props';

export type { TerminalViewProps } from './terminal-view-props';

const RustTerminalView = React.lazy(() => import('./RustTerminalView').then(module => ({ default: module.RustTerminalView })));

export function TerminalView(props: TerminalViewProps) {
  const driver = nativeTerminalForApi(props.api);
  return driver ? <React.Suspense fallback={<View style={{ flex: 1 }} />}>
    <RustTerminalView {...props} driver={driver} />
  </React.Suspense> : <WebViewTerminalView {...props} />;
}
