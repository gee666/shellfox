// Compatibility prefix only. Preserve old values for rollback to the previous app.
const LEGACY_PREFIX = 'pi-manager.';
export function readAppStorage(storage: Pick<Storage, 'getItem' | 'setItem'>, key: string): string | null {
  const current = storage.getItem(key);
  if (current !== null || !key.startsWith('shellfox.')) return current;
  const previous = storage.getItem(LEGACY_PREFIX + key.slice('shellfox.'.length));
  if (previous !== null) {
    try { storage.setItem(key, previous); } catch { /* Reading still works when writes are disabled. */ }
  }
  return previous;
}
