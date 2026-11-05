import type { SessionDto, TabDto, TerminalProfileDto } from '../../shared/contracts';
import { parseWslUnc } from '../../shared/wsl-path';

/** A WSL guest absolute path is meaningful only while its distribution context is retained. */
export function filePanelScope(session: SessionDto | undefined, tab: TabDto | undefined, profiles: readonly TerminalProfileDto[]) {
  if (!session) return undefined;
  const wsl = session.shellId === 'wsl' || !!parseWslUnc(tab?.cwd ?? session.cwd) || tab?.profileId?.startsWith('wsl:')
    || profiles.some(profile => profile.id === tab?.profileId && profile.environment === 'wsl');
  return wsl ? { sessionId: session.id, ...(tab ? { tabId: tab.id } : {}) } : undefined;
}
