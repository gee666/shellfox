import type { HostMode, HostStatus } from '../shared/host-contracts';
import type { HostKeepAwake, HostSessionService } from './operations';

export function getHostStatus(mode: HostMode, startedAt: string, service: Pick<HostSessionService, 'getTerminalProfiles'>, keepAwake?: HostKeepAwake, desktopActions = false): HostStatus {
  const profiles = service.getTerminalProfiles();
  return { mode, pid: process.pid, startedAt, capabilities: {
    keepAwake: !!keepAwake && keepAwake.supported !== false,
    desktopActions,
    administrator: mode === 'desktop' && profiles.ok && profiles.value.profiles.some(profile => profile.available && profile.elevation === 'administrator'),
  } };
}
