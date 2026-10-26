import { fireEvent, screen } from '@testing-library/react';
import type { SettingsTab } from './SettingsPanel';

export function openSettingsTab(name: SettingsTab) {
  const tab = screen.getByRole('tab', { name: new RegExp(`^${name}(?:[, ]|$)`) });
  if (tab.getAttribute('aria-selected') !== 'true') fireEvent.click(tab);
  return tab;
}
export function settingsControl(label: string) {
  openSettingsTab(label === 'Add agent' ? 'Agents' : label === 'Default shell' || label === 'Python path' ? 'Terminals' : 'General');
  return screen.getByLabelText(label);
}
