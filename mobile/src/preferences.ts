import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { remoteHostnameSchema, remotePeerIdSchema } from '../../src/shared/remote-schemas';
import { colorSchema, customColors } from './color-settings';

export const DEFAULT_MODIFIER_RELEASE_DELAY_MS = 1000;
export const MIN_MODIFIER_RELEASE_DELAY_MS = 250;
export const MAX_MODIFIER_RELEASE_DELAY_MS = 3000;
export const MODIFIER_RELEASE_DELAY_STEP_MS = 250;

export const preferencesSchema = z
  .object({
    hostname: remoteHostnameSchema.default('shellfox-mobile'),
    remembered: z.array(remotePeerIdSchema).max(1000).default([]),
    fontSize: z.number().int().min(10).max(24).default(14),
    autoUpdate: z.boolean().default(true),
    modifierReleaseDelayMs: z.number().int()
      .min(MIN_MODIFIER_RELEASE_DELAY_MS).max(MAX_MODIFIER_RELEASE_DELAY_MS)
      .multipleOf(MODIFIER_RELEASE_DELAY_STEP_MS).optional(),
    // Missing color fields retain the desktop defaults without rewriting older preferences.
    accentColor: colorSchema.optional(),
    backgroundColor: colorSchema.optional(),
    customAccentColors: z.array(colorSchema).max(1000).transform(values => customColors('accent', values)).optional(),
    customBackgroundColors: z.array(colorSchema).max(1000).transform(values => customColors('background', values)).optional(),
  })
  .strict();
export type Preferences = z.infer<typeof preferencesSchema>;
export const defaultPreferences: Preferences = {
  hostname: 'shellfox-mobile', remembered: [], fontSize: 14, autoUpdate: true,
  modifierReleaseDelayMs: DEFAULT_MODIFIER_RELEASE_DELAY_MS,
};
const key = 'shellfox.mobile.preferences.v1';
export async function loadPreferences(): Promise<Preferences> {
  const value = await AsyncStorage.getItem(key);
  if (value === null) return defaultPreferences;
  try {
    return { ...defaultPreferences, ...preferencesSchema.parse(JSON.parse(value)) };
  } catch {
    return defaultPreferences;
  }
}
export async function savePreferences(value: Preferences): Promise<void> {
  await AsyncStorage.setItem(key, JSON.stringify(preferencesSchema.parse(value)));
}
