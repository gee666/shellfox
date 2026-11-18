import { describe, expect, it } from 'vitest';
import { isAutomationView, isViewableTab } from './terminal-view';

describe('automation display marker compatibility', () => {
  it('uses the committed automation-pipe marker for open and closed read-only views', () => {
    expect(isAutomationView({ profileId: 'automation-pipe' })).toBe(true);
    expect(isViewableTab({ profileId: 'automation-pipe', lifecycle: 'open' })).toBe(true);
    expect(isViewableTab({ profileId: 'automation-pipe', lifecycle: 'closed' })).toBe(true);
  });

  it('does not classify ordinary profiles or arbitrary names as automation views', () => {
    for (const profileId of ['cmd', 'pwsh', 'automation', 'not-automation-pipe', null, undefined]) {
      expect(isAutomationView({ profileId })).toBe(false);
      expect(isViewableTab({ profileId, lifecycle: 'closed' })).toBe(false);
    }
    expect(isViewableTab({ profileId: 'cmd', lifecycle: 'open' })).toBe(true);
  });
});
