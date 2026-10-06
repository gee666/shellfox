import { expect, it } from 'vitest';
import { defaultSettings, upgradeBundledRules } from './defaults';
const old = ['@earendil-works/pi-coding-agent/dist/cli.js', '@mariozechner/pi-coding-agent/dist/cli.js'];
const saved = () => structuredClone(defaultSettings);
it('covers Pi bundle scripts and native Pi without classifying all Node programs as agents', () => {
  const rule = defaultSettings.processRules.find(r => r.label === 'Pi Node launcher')!;
  expect(rule.scriptPathSuffixes).toContain('@earendil-works/pi-coding-agent/dist/bundle/cli.js');
  expect(rule.executableBasenames).toEqual(['node.exe', 'node']);
  const native = defaultSettings.processRules.find(r => r.label === 'Native agents')!;
  expect(native.executableBasenames).toEqual(expect.arrayContaining(['pi.exe', 'pi', 'claude.exe', 'claude', 'codex.exe', 'codex']));
  expect(native.scriptPathSuffixes).toEqual([]);
});
it.each([['node.exe'], ['node'], ['node.exe', 'node']])('upgrades untouched old Pi names %j including embedded settings without enabling disabled rules', (...names: string[]) => {
  for (const suffixes of [old, defaultSettings.processRules[0]!.scriptPathSuffixes]) {
    const value = saved();
    value.adapterId = 'embedded-pty';
    value.processRules[0] = { ...value.processRules[0]!, executableBasenames: names, scriptPathSuffixes: suffixes, enabled: false };
    const result = upgradeBundledRules(value);
    expect(result.processRules[0]).toEqual({ ...defaultSettings.processRules[0], enabled: false });
    expect(upgradeBundledRules(result)).toBe(result);
  }
});
it.each([
  ['claude.exe', 'codex.exe', 'opencode.exe'],
  ['claude', 'codex', 'opencode'],
  ['claude.exe', 'claude', 'codex.exe', 'codex', 'opencode.exe', 'opencode'],
])('upgrades untouched native-agent defaults %j to detect native Pi too', (...names: string[]) => {
  const value = saved(), index = value.processRules.findIndex(r => r.label === 'Native agents');
  value.processRules[index] = { ...value.processRules[index]!, enabled: false, executableBasenames: names };
  const result = upgradeBundledRules(value);
  expect(result.processRules[index]).toEqual({ ...defaultSettings.processRules[index], enabled: false });
  expect(upgradeBundledRules(result)).toBe(result);
});
it('does not overwrite custom names, paths, suffixes, labels or removed rules', () => {
  for (const patch of [{ label: 'My pi' }, { executablePaths: ['C:\\custom\\node.exe'] }, { scriptPathSuffixes: ['my-agent/cli.js'] }, { executableBasenames: ['my-node.exe'] }]) {
    const value = saved(); value.processRules[0] = { ...value.processRules[0]!, ...patch };
    expect(upgradeBundledRules(value)).toBe(value);
  }
  const value = saved(); value.processRules = value.processRules.filter(r => r.label !== 'Native agents');
  expect(upgradeBundledRules(value)).toBe(value);
  expect(upgradeBundledRules(defaultSettings)).toBe(defaultSettings);
});
