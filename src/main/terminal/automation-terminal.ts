import type { TerminalProfileDto } from '../../shared/contracts';
import type { SessionRecord } from '../models';

export const AUTOMATION_PROFILE_ID = 'automation-pipe';
// Stored in the existing executable field as a durable non-shell session marker.
// This profile is never discovered or passed to the shell launcher.
export const automationProfile: TerminalProfileDto = {
  id: AUTOMATION_PROFILE_ID, label: 'Automation', environment: 'local',
  executable: 'shellfox:automation-pipe', args: [], distro: null, available: true,
};
export function isAutomationSession(session: SessionRecord): boolean {
  return session.shellExecutable === automationProfile.executable;
}
