import type { TabDto } from './contracts';

export function isAutomationView(tab: Pick<TabDto, 'profileId'>): boolean {
  return tab.profileId === 'automation-pipe';
}

/** Completed automation runs keep their replay until the host retires the view. */
export function isViewableTab(tab: Pick<TabDto, 'profileId' | 'lifecycle'>): boolean {
  return tab.lifecycle !== 'closed' || isAutomationView(tab);
}
