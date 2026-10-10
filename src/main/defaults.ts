import type { SettingsDto, NativeProbe, ExplorerIntegrationDto, CliIntegrationDto } from '../shared/contracts';
const linux = process.platform === 'linux';
export const defaultSettings: SettingsDto = {
  version: 1, pythonPath: null, accentColor: '#ec4899', backgroundColor: '#111016', adapterId: linux ? 'gnome-terminal' : 'windows-terminal', shellId: linux ? 'bash' : 'pwsh',
  shellExecutable: null, historyPageSize: 20,
  processRules: [
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5870', label: 'Pi', enabled: true, processNames: ['pi', 'pi.exe'], executableBasenames: ['node.exe', 'node', 'nodejs', 'bun.exe', 'bun'], executablePaths: [], scriptPathSuffixes: ['@earendil-works/pi-coding-agent/dist/cli.js', '@mariozechner/pi-coding-agent/dist/cli.js', '@earendil-works/pi-coding-agent/dist/bundle/cli.js', '@mariozechner/pi-coding-agent/dist/bundle/cli.js'] },
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5871', label: 'Claude', enabled: true, processNames: ['claude', 'claude.exe'], executableBasenames: ['node.exe', 'node', 'nodejs', 'bun.exe', 'bun'], executablePaths: [], scriptPathSuffixes: ['@anthropic-ai/claude-code/cli.js'] },
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5872', label: 'Codex', enabled: true, processNames: ['codex', 'codex.exe'], executableBasenames: ['node.exe', 'node', 'nodejs', 'bun.exe', 'bun'], executablePaths: [], scriptPathSuffixes: ['@openai/codex/bin/codex.js'] },
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5874', label: 'OpenCode', enabled: true, processNames: ['opencode', 'opencode.exe'], executableBasenames: ['node.exe', 'node', 'nodejs', 'bun.exe', 'bun'], executablePaths: [], scriptPathSuffixes: ['opencode-ai/bin/opencode'] },
  ],
};
const sameNames = (a: string[], b: string[]): boolean => JSON.stringify([...a].sort()) === JSON.stringify([...b].sort());
const nativeId = 'f919fb1a-fb03-4a93-8b9b-1cde465d5873';
const nameVariants = (names: string[]) => [names, names.map(n => n + '.exe'), names.flatMap(n => [n, n + '.exe'])];
// Existing per-agent toggles win, even when Native agents was enabled. If only
// the native rule existed, its toggle supplies the missing per-agent rules.
// Recognize shipped values only. Customized legacy rules become editable custom
// rules rather than being silently discarded or hidden behind a builtin switch.
export function upgradeBundledRules(settings: SettingsDto): SettingsDto {
  const oldPiSuffixes = ['@earendil-works/pi-coding-agent/dist/cli.js', '@mariozechner/pi-coding-agent/dist/cli.js'];
  const native = settings.processRules.find(r => r.id === nativeId);
  const nativeDefaults = native?.label === 'Native agents' && !native.executablePaths.length &&
    !native.scriptPathSuffixes.length && !native.processNames &&
    [...nameVariants(['claude', 'codex', 'opencode']), ...nameVariants(['pi', 'claude', 'codex', 'opencode'])]
      .some(names => sameNames(native.executableBasenames, names));
  const missing = nativeDefaults ? defaultSettings.processRules.filter(bundled =>
    !settings.processRules.some(r => r.id === bundled.id) && bundled.processNames!.some(name => native!.executableBasenames.includes(name))) : [];
  // A full custom list must not become invalid or lose native coverage on upgrade.
  const consolidateNative = nativeDefaults && settings.processRules.length - 1 + missing.length <= 100;
  let changed = false;
  const ids = new Set(settings.processRules.map(r => r.id));
  const asCustom = (rule: SettingsDto['processRules'][number], key: string, bundledLabel: string) => {
    let id = 'custom-bundled-' + key.toLowerCase() + '-legacy';
    for (let n = 2; ids.has(id); n++) id = 'custom-bundled-' + key.toLowerCase() + '-legacy' + n;
    ids.add(id);
    changed = true;
    return { ...rule, id, label: rule.label === bundledLabel ? 'Custom ' + key : rule.label };
  };
  const rules = settings.processRules.flatMap(rule => {
    if (rule.id === nativeId) {
      changed = true;
      return consolidateNative ? [] : [asCustom(rule, 'native-agents', 'Native agents')];
    }
    const bundled = defaultSettings.processRules.find(r => r.id === rule.id);
    if (!bundled) return [rule];
    const legacyLabel = bundled.label + ' Node launcher';
    const shipped = [bundled.label, legacyLabel].includes(rule.label) && !rule.executablePaths.length &&
      (!rule.processNames || sameNames(rule.processNames, bundled.processNames!)) &&
      [...nameVariants(['node']), bundled.executableBasenames].some(names => sameNames(rule.executableBasenames, names)) &&
      (sameNames(rule.scriptPathSuffixes, bundled.scriptPathSuffixes) || bundled.label === 'Pi' && sameNames(rule.scriptPathSuffixes, oldPiSuffixes));
    if (!shipped) return [asCustom(rule, bundled.label, legacyLabel)];
    const updated = { ...structuredClone(bundled), enabled: rule.enabled };
    if (JSON.stringify(rule) === JSON.stringify(updated)) return [rule];
    changed = true;
    return [updated];
  });
  if (consolidateNative) rules.push(...missing.map(bundled => ({ ...structuredClone(bundled), enabled: native!.enabled })));
  return changed ? { ...settings, processRules: rules } : settings;
}
export const unavailableProbe = (reason: string): NativeProbe => ({
  platform: process.platform, arch: process.arch, adapterId: linux ? 'gnome-terminal' : 'windows-terminal', available: false, terminalVersion: null,
  capabilities: { createWindow: false, addTab: false, focusWindow: false, activateTab: false, splitPane: false, attachExisting: false, closeTerminal: false, commandExitStatus: false, processTracking: false, explorerContextMenu: false },
  shells: [], reasons: [reason],
});
export const unavailableCli: CliIntegrationDto = { supported: false, installed: false, command: 'shellfox', reason: 'Shellfox CLI integration is unavailable.' };
export const unavailableExplorer: ExplorerIntegrationDto = { supported: false, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: 'Native integration is unavailable' };
