import { beforeEach, describe, expect, it, vi } from 'vitest';

const storage = vi.hoisted(() => ({ getItem: vi.fn<() => Promise<string | null>>(), setItem: vi.fn<(_key: string, _value: string) => Promise<void>>() }));
vi.mock('@react-native-async-storage/async-storage', () => ({ default: storage }));
import { defaultPreferences, loadPreferences, preferencesSchema, savePreferences } from './preferences';

beforeEach(() => { vi.resetAllMocks(); storage.getItem.mockResolvedValue(null); storage.setItem.mockResolvedValue(); });

describe('mobile preferences', () => {
  it('uses compact defaults on first launch', async () => {
    expect(await loadPreferences()).toEqual({ hostname: 'shellfox-mobile', remembered: [], fontSize: 14, autoUpdate: true });
    expect(storage.getItem).toHaveBeenCalledWith('shellfox.mobile.preferences.v1');
  });
  it('round trips a renamed device, remembered host and font setting', async () => {
    const value = { hostname: 'phone-2', remembered: ['host:123', 'host.two'], fontSize: 20, autoUpdate: false };
    await savePreferences(value);
    const [key, json] = storage.setItem.mock.calls[0];
    expect(key).toBe('shellfox.mobile.preferences.v1');
    storage.getItem.mockResolvedValue(json);
    expect(await loadPreferences()).toEqual(value);
  });
  it('defaults automatic updates on for older preferences and rejects non-boolean settings', () => {
    expect(preferencesSchema.parse({ hostname: 'old-phone' }).autoUpdate).toBe(true);
    expect(preferencesSchema.parse({ autoUpdate: false }).autoUpdate).toBe(false);
    expect(preferencesSchema.safeParse({ autoUpdate: 'false' }).success).toBe(false);
  });
  it('fills defaults for older partial preferences', async () => {
    storage.getItem.mockResolvedValue(JSON.stringify({ hostname: 'old-phone' }));
    expect(await loadPreferences()).toEqual({ ...defaultPreferences, hostname: 'old-phone' });
  });
  it.each(['not json', 'null', '[]', '{"fontSize":25}', '{"fontSize":9}', '{"fontSize":14.5}', '{"hostname":"bad/name"}', '{"remembered":["../host"]}', '{"authKey":"do-not-store"}'])('recovers safely from invalid persisted value %s', async value => {
    storage.getItem.mockResolvedValue(value);
    expect(await loadPreferences()).toEqual(defaultPreferences);
  });
  it.each([10, 24])('accepts font size boundary %i', async fontSize => {
    await savePreferences({ ...defaultPreferences, fontSize });
    expect(storage.setItem).toHaveBeenCalledOnce();
  });
  it('rejects invalid writes before touching storage', async () => {
    await expect(savePreferences({ ...defaultPreferences, hostname: 'not a hostname' })).rejects.toThrow();
    expect(storage.setItem).not.toHaveBeenCalled();
    expect(preferencesSchema.safeParse({ ...defaultPreferences, remembered: Array.from({ length: 1001 }, () => 'host') }).success).toBe(false);
  });
  it('persists color choices and independent last-three custom swatches', async () => {
    await savePreferences({
      ...defaultPreferences,
      accentColor: '#AABBCC',
      backgroundColor: '#ffffff',
      customAccentColors: ['#112233', '#445566', '#EC4899', '#112233', '#778899', '#AABBCC'],
      customBackgroundColors: ['#010203', '#000000', '#010203'],
    });
    const [, json] = storage.setItem.mock.calls[0];
    storage.getItem.mockResolvedValue(json);
    expect(await loadPreferences()).toEqual({
      ...defaultPreferences,
      accentColor: '#aabbcc',
      backgroundColor: '#ffffff',
      customAccentColors: ['#445566', '#778899', '#aabbcc'],
      customBackgroundColors: ['#010203'],
    });
  });
  it.each([
    { accentColor: 'red' }, { backgroundColor: '#fff' },
    { customAccentColors: ['#123456', 'bad'] }, { customBackgroundColors: ['#12345g'] },
  ])('rejects malformed color preferences %j', async patch => {
    await expect(savePreferences({ ...defaultPreferences, ...patch })).rejects.toThrow();
    expect(storage.setItem).not.toHaveBeenCalled();
    storage.getItem.mockResolvedValue(JSON.stringify({ ...defaultPreferences, ...patch }));
    expect(await loadPreferences()).toEqual(defaultPreferences);
  });
  it('does not hide storage read or write failures', async () => {
    storage.getItem.mockRejectedValue(new Error('read failed'));
    await expect(loadPreferences()).rejects.toThrow('read failed');
    storage.setItem.mockRejectedValue(new Error('disk full'));
    await expect(savePreferences(defaultPreferences)).rejects.toThrow('disk full');
  });
});
