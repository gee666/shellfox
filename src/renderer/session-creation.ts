import type { ManagerApi, ManagerSnapshot, TerminalProfileDto } from '../shared/contracts';
import { getTerminalApi } from './api';
import type { ManagerClient } from './store';

export interface SessionCreationTarget {
  machineId: string; name: string; api: ManagerApi; client: ManagerClient; profileId: string | null; platform: string;
}
export function canCreateSession(snapshot: ManagerSnapshot | null, api: ManagerApi, profiles: TerminalProfileDto[], defaultProfileId: string | null): boolean {
  if (!snapshot?.probe.available || !snapshot.probe.capabilities.createWindow) return false;
  if (snapshot.probe.adapterId !== 'embedded-pty') return true;
  const selected = snapshot.settings.terminalProfileId ?? defaultProfileId;
  return !!getTerminalApi(api) && profiles.some(profile => profile.available && (snapshot.probe.platform === 'win32' || profile.id === selected));
}
