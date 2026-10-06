import { expect, it } from 'vitest';
import { defaultSettings, upgradeBundledRules } from './defaults';
import { settingsSchema } from '../shared/schemas';
import type { ProcessRule } from '../shared/contracts';
const oldPi = ['@earendil-works/pi-coding-agent/dist/cli.js', '@mariozechner/pi-coding-agent/dist/cli.js'];
const saved = () => structuredClone(defaultSettings);
const legacy = () => ({ ...saved(), processRules: saved().processRules.map(({ processNames: _, ...r }) => ({ ...r, label: r.label + ' Node launcher', executableBasenames: ['node.exe'] })) });
const native = (patch: Partial<ProcessRule> = {}): ProcessRule => ({ id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5873', label: 'Native agents', enabled: true, executableBasenames: ['pi', 'pi.exe', 'claude', 'claude.exe', 'codex', 'codex.exe', 'opencode', 'opencode.exe'], executablePaths: [], scriptPathSuffixes: [], ...patch });
it('has four unified switches, trusted names and Node/Bun script fallbacks', () => {
  expect(defaultSettings.processRules.map(r => r.label)).toEqual(['Pi', 'Claude', 'Codex', 'OpenCode']);
  for (const rule of defaultSettings.processRules) {
    expect(rule.processNames).toHaveLength(2);
    expect(rule.executableBasenames).toEqual(expect.arrayContaining(['node', 'node.exe', 'nodejs', 'bun', 'bun.exe']));
    expect(rule.scriptPathSuffixes.length).toBeGreaterThan(0);
  }
  expect(defaultSettings.processRules[0].scriptPathSuffixes).toContain('@earendil-works/pi-coding-agent/dist/bundle/cli.js');
  expect(settingsSchema.safeParse(defaultSettings).success).toBe(true);
  expect(upgradeBundledRules(defaultSettings)).toBe(defaultSettings);
});
it.each([['node.exe'], ['node'], ['node.exe', 'node']])('migrates old runtimes %j and Pi versions without enabling disabled switches', (...names: string[]) => {
  for (const suffixes of [oldPi, defaultSettings.processRules[0].scriptPathSuffixes]) {
    const value = legacy(); value.adapterId = 'embedded-pty';
    value.processRules[0] = { ...value.processRules[0], enabled: false, executableBasenames: names, scriptPathSuffixes: suffixes };
    value.processRules.push(native());
    const result = upgradeBundledRules(value);
    expect(result.processRules).toEqual(defaultSettings.processRules.map((r, i) => ({ ...r, enabled: i !== 0 })));
    expect(upgradeBundledRules(result)).toBe(result);
  }
});
it.each([
  ['claude.exe', 'codex.exe', 'opencode.exe'], ['claude', 'codex', 'opencode'],
  ['claude.exe', 'claude', 'codex.exe', 'codex', 'opencode.exe', 'opencode'],
  native().executableBasenames,
])('removes untouched Native agents %j, using per-agent toggle precedence', (...names: string[]) => {
  for (const enabled of [true, false]) {
    const value = legacy(); value.processRules[1].enabled = false;
    value.processRules.push(native({ enabled, executableBasenames: names }));
    const result = upgradeBundledRules(value);
    expect(result.processRules).toEqual(defaultSettings.processRules.map((r, i) => ({ ...r, enabled: i !== 1 })));
    expect(upgradeBundledRules(result)).toBe(result);
  }
});
it('uses the native toggle for agents with no existing per-agent rule', () => {
  for (const enabled of [true, false]) {
    const value = saved(); value.processRules = [native({ enabled })];
    expect(upgradeBundledRules(value).processRules).toEqual(defaultSettings.processRules.map(r => ({ ...r, enabled })));
  }
  const value = saved(); value.processRules = [native({ executableBasenames: ['claude.exe', 'codex.exe', 'opencode.exe'] })];
  expect(upgradeBundledRules(value).processRules.map(r => r.label)).toEqual(['Claude', 'Codex', 'OpenCode']);
});
it('preserves customized legacy bundled rules as editable custom rules', () => {
  for (const base of [legacy().processRules[0], native()]) {
    for (const patch of [{ label: 'My agent' }, { executablePaths: ['C:\\custom\\agent.exe'] }, { scriptPathSuffixes: ['my-agent/cli.js'] }, { executableBasenames: ['my-agent.exe'] }, { processNames: ['my-agent'] }]) {
      const value = saved(); value.processRules = [{ ...base, ...patch, enabled: false }];
      const result = upgradeBundledRules(value), rule = result.processRules[0];
      expect(rule).toMatchObject({ ...base, ...patch, id: expect.stringMatching(/^custom-/), label: expect.any(String), enabled: false });
      expect(settingsSchema.safeParse(result).success).toBe(true);
      expect(upgradeBundledRules(result)).toBe(result);
    }
  }
});
it('preserves unrelated custom rules, missing rules, and colliding custom IDs', () => {
  const value = saved(), custom = native({ id: 'custom-bundled-native-agents-legacy', label: 'mine' });
  value.processRules = [custom, native({ executableBasenames: ['other'] })];
  const result = upgradeBundledRules(value);
  expect(result.processRules[0]).toBe(custom);
  expect(result.processRules[1].id).not.toBe(custom.id);
  expect(settingsSchema.safeParse(result).success).toBe(true);
  value.processRules = [custom]; expect(upgradeBundledRules(value)).toBe(value);
  value.processRules = []; expect(upgradeBundledRules(value)).toBe(value);
});
it('preserves native coverage as custom when adding unified switches would exceed the rule limit', () => {
  const value = saved(); value.processRules = [native(), ...Array.from({ length: 99 }, (_, i) => native({ id: `custom-test-${i}`, label: 'custom' }))];
  const result = upgradeBundledRules(value);
  expect(result.processRules).toHaveLength(100);
  expect(result.processRules[0]).toMatchObject({ id: expect.stringMatching(/^custom-/), executableBasenames: native().executableBasenames });
  expect(settingsSchema.safeParse(result).success).toBe(true);
  expect(upgradeBundledRules(result)).toBe(result);
});
it('keeps the new field optional in stored and custom UI rules', () => {
  expect(settingsSchema.safeParse(legacy()).success).toBe(true);
  for (const patch of [{ executableBasenames: ['my-agent', 'my-agent.exe'] }, { executableBasenames: [], executablePaths: ['/opt/my-agent'] }]) {
    const value = saved(); value.processRules = [native({ id: 'custom-test-123', label: 'custom', ...patch })];
    expect(settingsSchema.safeParse(value).success).toBe(true);
    expect(upgradeBundledRules(value)).toBe(value);
  }
});
