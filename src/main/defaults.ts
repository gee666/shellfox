import type { SettingsDto, NativeProbe, ExplorerIntegrationDto, CliIntegrationDto } from '../shared/contracts';
const linux = process.platform === 'linux';
export const defaultSettings: SettingsDto = {
  version: 1, pythonPath: null, accentColor: '#ec4899', adapterId: linux ? 'gnome-terminal' : 'windows-terminal', shellId: linux ? 'bash' : 'pwsh',
  shellExecutable: null, historyPageSize: 20,
  processRules: [
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5870', label: 'Pi Node launcher', enabled: true, executableBasenames: ['node.exe', 'node'], executablePaths: [], scriptPathSuffixes: ['@earendil-works/pi-coding-agent/dist/cli.js', '@mariozechner/pi-coding-agent/dist/cli.js', '@earendil-works/pi-coding-agent/dist/bundle/cli.js', '@mariozechner/pi-coding-agent/dist/bundle/cli.js'] },
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5871', label: 'Claude Node launcher', enabled: true, executableBasenames: ['node.exe', 'node'], executablePaths: [], scriptPathSuffixes: ['@anthropic-ai/claude-code/cli.js'] },
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5872', label: 'Codex Node launcher', enabled: true, executableBasenames: ['node.exe', 'node'], executablePaths: [], scriptPathSuffixes: ['@openai/codex/bin/codex.js'] },
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5874', label: 'OpenCode Node launcher', enabled: true, executableBasenames: ['node.exe', 'node'], executablePaths: [], scriptPathSuffixes: ['opencode-ai/bin/opencode'] },
    // Native names have no script filter. Adding pi.exe to the Node+script rule alone would never match it.
    { id: 'f919fb1a-fb03-4a93-8b9b-1cde465d5873', label: 'Native agents', enabled: true, executableBasenames: ['pi.exe', 'pi', 'claude.exe', 'claude', 'codex.exe', 'codex', 'opencode.exe', 'opencode'], executablePaths: [], scriptPathSuffixes: [] },
  ],
};
const sameNames = (a: string[], b: string[]): boolean => JSON.stringify([...a].sort()) === JSON.stringify([...b].sort());
// Recognize both legacy platform defaults and the names expanded on embedded migration.
// Preserve custom names, paths, suffixes, labels, removed rules and every enabled toggle.
export function upgradeBundledRules(settings: SettingsDto): SettingsDto {
  const oldPiSuffixes = ['@earendil-works/pi-coding-agent/dist/cli.js', '@mariozechner/pi-coding-agent/dist/cli.js'];
  let changed = false;
  const rules = settings.processRules.map(rule => {
    const bundled = defaultSettings.processRules.find(r => r.id === rule.id && r.label === rule.label);
    if (!bundled || rule.executablePaths.length) return rule;
    const legacyNames = bundled.label === 'Native agents' ? ['claude.exe', 'codex.exe', 'opencode.exe'] : ['node.exe'];
    const variants = [legacyNames, legacyNames.map(n => n.replace(/\.exe$/, '')), legacyNames.flatMap(n => [n, n.replace(/\.exe$/, '')]), bundled.executableBasenames];
    const suffixesMatch = sameNames(rule.scriptPathSuffixes, bundled.scriptPathSuffixes) || bundled.label === 'Pi Node launcher' && sameNames(rule.scriptPathSuffixes, oldPiSuffixes);
    if (!variants.some(names => sameNames(rule.executableBasenames, names)) || !suffixesMatch) return rule;
    if (JSON.stringify(rule.executableBasenames) === JSON.stringify(bundled.executableBasenames) && JSON.stringify(rule.scriptPathSuffixes) === JSON.stringify(bundled.scriptPathSuffixes)) return rule;
    changed = true;
    return { ...rule, executableBasenames: [...bundled.executableBasenames], scriptPathSuffixes: [...bundled.scriptPathSuffixes] };
  });
  return changed ? { ...settings, processRules: rules } : settings;
}
export const unavailableProbe = (reason: string): NativeProbe => ({
  platform: process.platform, arch: process.arch, adapterId: linux ? 'gnome-terminal' : 'windows-terminal', available: false, terminalVersion: null,
  capabilities: { createWindow: false, addTab: false, focusWindow: false, activateTab: false, splitPane: false, attachExisting: false, closeTerminal: false, commandExitStatus: false, processTracking: false, explorerContextMenu: false },
  shells: [], reasons: [reason],
});
export const unavailableCli: CliIntegrationDto = { supported: false, installed: false, command: 'shellfox start <path>', reason: 'Shellfox CLI integration requires Windows.' };
export const unavailableExplorer: ExplorerIntegrationDto = { supported: false, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: 'Native integration is unavailable' };
