import { expect, it } from 'vitest';
import { idSchema, processRuleSchema, settingsSchema } from './schemas';
import { defaultSettings } from '../main/defaults';
it('accepts the renderer-requested custom rule ids without relaxing session/tab identities', () => {
  const id = 'custom-aider-12345678-1234-4234-8234-123456789012';
  const rule = { id, label: 'aider', enabled: true, executableBasenames: ['aider', 'aider.exe'], executablePaths: [], scriptPathSuffixes: [] };
  expect(processRuleSchema.safeParse(rule).success).toBe(true);
  expect(settingsSchema.safeParse({ ...defaultSettings, processRules: [...defaultSettings.processRules, rule] }).success).toBe(true);
  expect(idSchema.safeParse(id).success).toBe(false);
  for (const bad of ['aider', 'custom-aider', 'custom-../evil-id', 'custom-agent-\n', 'custom-' + 'x'.repeat(120) + '-id']) expect(processRuleSchema.safeParse({ ...rule, id: bad }).success).toBe(false);
  expect(settingsSchema.safeParse({ ...defaultSettings, processRules: [rule, rule] }).success).toBe(false);
});
