import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { remoteHostnameSchema, remotePeerIdSchema } from '../../src/shared/remote-schemas';
import { colorSchema, customColors } from './color-settings';

export const preferencesSchema = z
  .object({
    hostname: remoteHostnameSchema.default('shellfox-mobile'),
    remembered: z.array(remotePeerIdSchema).max(1000).default([]),
    fontSize: z.number().int().min(10).max(24).default(14),
    // Missing color fields retain the desktop defaults without rewriting older preferences.
    accentColor: colorSchema.optional(),
    backgroundColor: colorSchema.optional(),
    customAccentColors: z.array(colorSchema).max(1000).transform(values => customColors('accent', values)).optional(),
    customBackgroundColors: z.array(colorSchema).max(1000).transform(values => customColors('background', values)).optional(),
  })
  .strict();
export type Preferences = z.infer<typeof preferencesSchema>;
export const defaultPreferences: Preferences = { hostname: 'shellfox-mobile', remembered: [], fontSize: 14 };
const key = 'shellfox.mobile.preferences.v1';
export async function loadPreferences(): Promise<Preferences> {
  const value = await AsyncStorage.getItem(key);
  if (value === null) return defaultPreferences;
  try {
    return preferencesSchema.parse(JSON.parse(value));
  } catch {
    return defaultPreferences;
  }
}
export async function savePreferences(value: Preferences): Promise<void> {
  await AsyncStorage.setItem(key, JSON.stringify(preferencesSchema.parse(value)));
}
