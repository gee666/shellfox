import React from 'react';
import { nativeTerminalForApi } from '../rust-client/native-terminal';
import { Note } from '../ui';
import { RustTerminalView } from './RustTerminalView';
import type { TerminalViewProps } from './terminal-view-props';

export type { TerminalViewProps } from './terminal-view-props';

export function TerminalView({ api, ...props }: TerminalViewProps) {
  const driver = nativeTerminalForApi(api);
  if (!driver) return <Note error>This terminal has no native connection.</Note>;
  return <RustTerminalView {...props} driver={driver} />;
}
