import { expect, it } from 'vitest';
import { readAppStorage } from './storage-upgrade';

it('copies old app storage on read, keeps rollback values and prefers Shellfox values', () => {
  const values = new Map([['pi-manager.sidebarWidth', '280'], ['pi-manager.otherSetting', 'saved']]);
  const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value); } };
  expect(readAppStorage(storage, 'shellfox.sidebarWidth')).toBe('280');
  expect(values.get('shellfox.sidebarWidth')).toBe('280');
  expect(values.get('pi-manager.sidebarWidth')).toBe('280');
  values.set('shellfox.sidebarWidth', '320');
  expect(readAppStorage(storage, 'shellfox.sidebarWidth')).toBe('320');
  expect(readAppStorage(storage, 'shellfox.otherSetting')).toBe('saved');
  expect(readAppStorage(storage, 'shellfox.missing')).toBeNull();
  expect(readAppStorage(storage, 'foreign.otherSetting')).toBeNull();
});
it('returns the old preference even if storage writes are denied', () => {
  expect(readAppStorage({ getItem: key => key === 'pi-manager.sidebarWidth' ? '260' : null, setItem: () => { throw new Error('denied'); } }, 'shellfox.sidebarWidth')).toBe('260');
});
