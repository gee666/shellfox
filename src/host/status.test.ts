import { expect, it } from 'vitest';
import { success, failure, type TerminalProfileDto } from '../shared/contracts';
import { getHostStatus } from './status';

const elevated: TerminalProfileDto = { id: 'administrator-powershell', label: 'Administrator PowerShell', executable: 'powershell.exe', args: [], environment: 'local', distro: null, available: true, elevation: 'administrator' };
it('reports administrator support only for a desktop with an available cached elevated profile', () => {
  const service = (profiles: TerminalProfileDto[]) => ({ getTerminalProfiles: () => success({ profiles, defaultProfileId: null, lifetime: 'app-owned' as const, shellSurvival: false as const }) });
  const startedAt = new Date().toISOString();
  expect(getHostStatus('desktop', startedAt, service([])).capabilities.administrator).toBe(false);
  expect(getHostStatus('desktop', startedAt, service([{ ...elevated, available: false }])).capabilities.administrator).toBe(false);
  expect(getHostStatus('desktop', startedAt, service([elevated])).capabilities.administrator).toBe(true);
  expect(getHostStatus('headless', startedAt, service([elevated])).capabilities.administrator).toBe(false);
  expect(getHostStatus('desktop', startedAt, { getTerminalProfiles: () => failure('NATIVE_UNAVAILABLE', 'unavailable') }).capabilities.administrator).toBe(false);
});
