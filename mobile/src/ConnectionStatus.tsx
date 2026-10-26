import React from 'react';
import { Text } from 'react-native';
import type { ShellfoxModel } from './useShellfox';
import { connectionLabel, connectionSummary } from './connection-status';
import { useTheme } from './theme';

export function ConnectionStatus({ model, peerId, terminalState }: {
  model: Pick<ShellfoxModel, 'machines' | 'network'>;
  peerId?: string;
  terminalState?: 'attaching' | 'attached' | 'closed' | 'reconnecting';
}) {
  const { colors } = useTheme();
  const machine = model.machines.find(item => item.peer.id === peerId);
  let text = machine ? connectionLabel(machine.state.status)
    : connectionSummary(model.machines.map(item => item.state.status), model.network.status === 'starting');
  if (machine?.state.status === 'connected' && terminalState === 'attaching') text = 'connecting...';
  if (machine?.state.status === 'connected' && terminalState === 'reconnecting') text = 'reconnecting...';
  return <Text testID="connection-status" accessibilityLiveRegion="polite" accessibilityLabel={`Connection ${text}`}
    numberOfLines={1} style={{ color: text === 'connected' ? colors.green : colors.muted, fontSize: 11, flexShrink: 1, maxWidth: 140 }}>{text}</Text>;
}
