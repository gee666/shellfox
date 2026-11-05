import { expect, it } from 'vitest';
import { profiles, session } from '../test-fixtures';
import { filePanelScope } from './file-panel-scope';

it('uses host scope for local sessions and when no session is selected', () => {
  const local = session();
  expect(filePanelScope(local, local.tabs[0], profiles)).toBeUndefined();
  expect(filePanelScope(undefined, undefined, profiles)).toBeUndefined();
});

it.each(['profile', 'shell', 'unc', 'discovered profile'])('retains a distribution context from the %s', source => {
  const item = session();
  if (source === 'profile') item.tabs[0].profileId = 'wsl:Ubuntu';
  else if (source === 'shell') item.shellId = 'wsl';
  else if (source === 'unc') item.tabs[0].cwd = '\\\\wsl.localhost\\Ubuntu\\home\\guest';
  else item.tabs[0].profileId = 'custom-ubuntu';
  const known = source === 'discovered profile' ? [{ ...profiles[1], id: 'custom-ubuntu' }] : profiles;
  expect(filePanelScope(item, item.tabs[0], known)).toEqual({ sessionId: item.id, tabId: item.tabs[0].id });
});
