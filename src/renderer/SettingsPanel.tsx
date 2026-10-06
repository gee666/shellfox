import { useEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import type { CliIntegrationDto, ExplorerIntegrationDto, ProcessRule, SettingsDto, TerminalProfileDto } from '../shared/contracts';
import type { ManagerClient } from './store';
import { executablePathSchema, validateSettingsDraft } from './settings-controller';
import { Icon, useAction } from './components';

const BUILTIN_LABELS: Record<string, string> = { 'f919fb1a-fb03-4a93-8b9b-1cde465d5870': 'Pi', 'f919fb1a-fb03-4a93-8b9b-1cde465d5871': 'Claude', 'f919fb1a-fb03-4a93-8b9b-1cde465d5872': 'Codex', 'f919fb1a-fb03-4a93-8b9b-1cde465d5874': 'OpenCode' };
const BUILTIN_IDS = new Set(Object.keys(BUILTIN_LABELS));
const SWATCHES = ['#ec4899', '#a78bfa', '#60a5fa', '#2dd4bf', '#4ade80', '#fbbf24', '#fb923c', '#fc8397'];
const BACKGROUNDS = ['#111016', '#000000', '#1e1e2e', '#0f172a', '#ffffff', '#f5f5f7', '#fdf6e3'];
const nameKey = (value: string) => value.toLowerCase().replace(/\.exe$/, '');
// Windows paths ignore case and separator style; Linux paths remain case-sensitive.
const pathKey = (value: string) => /^[a-z]:[\\/]/i.test(value) ? value.replace(/\\/g, '/').toLowerCase() : value;
export function createAgentRule(value: string): ProcessRule {
  const input = value.trim();
  const isPath = executablePathSchema.safeParse(input).success;
  const name = isPath ? input.split(/[\\/]/).at(-1) || input : input.replace(/\.exe$/i, '');
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 60) || 'agent';
  return { id: `custom-${slug}-${crypto.randomUUID()}`, label: (isPath ? name : input).slice(0, 200), enabled: true, executableBasenames: isPath ? [] : [name, /\.exe$/i.test(input) ? input : `${name}.exe`], executablePaths: isPath ? [input] : [], scriptPathSuffixes: [] };
}
function ruleSummary(rule: ProcessRule) {
  return [rule.executableBasenames.length ? `Process name: ${rule.executableBasenames.join(', ')}` : '', ...rule.executablePaths.map(path => `Exact path: ${path}`)].filter(Boolean).join('; ');
}
export function SettingsPanel({ client, settings, explorer, cli, profiles, defaultProfileId }: {
  client: ManagerClient; settings: SettingsDto; explorer: ExplorerIntegrationDto; cli: CliIntegrationDto; profiles: TerminalProfileDto[]; defaultProfileId: string | null;
}) {
  const controller = client.settings;
  const state = useStore(controller.store);
  const draft = state.draft ?? settings;
  const [saved, setSaved] = useState(false);
  const saveError = state.error;
  const previousProps = useRef(settings);
  useEffect(() => {
    // Reopening uses the controller's latest draft, not the last modal props.
    // Standalone panels can still receive authoritative updates through props.
    if (!controller.store.getState().draft || previousProps.current !== settings) controller.observe(settings);
    previousProps.current = settings;
  }, [controller, settings]);
  useEffect(() => controller.mount(), [controller]);
  const [agentName, setAgentName] = useState('');
  const [agentError, setAgentError] = useState('');
  const [explorerState, setExplorerState] = useState(explorer);
  const [cliState, setCliState] = useState(cli);
  const action = useAction();
  const probe = useStore(client.store, state => state.snapshot?.probe);
  const linux = probe?.platform === 'linux';
  const errors = validateSettingsDraft(draft);
  const change = controller.update;
  function updateRule(id: string, patch: Partial<ProcessRule>) { change(current => ({ ...current, processRules: current.processRules.map(rule => rule.id === id ? { ...rule, ...patch } : rule) })); }
  useEffect(() => {
    if (state.dirty || !state.saved) { setSaved(false); return; }
    setSaved(true);
    const timer = setTimeout(() => setSaved(false), 1800);
    return () => clearTimeout(timer);
  }, [state.saved, state.dirty]);
  useEffect(() => { setExplorerState(explorer); }, [explorer]);
  useEffect(() => { setCliState(cli); }, [cli]);
  function addAgent() {
    const input = agentName.trim();
    const isPath = executablePathSchema.safeParse(input).success;
    if (!isPath && (!input || input.length > 200 || !nameKey(input) || /[\\/:\s\u0000-\u001f\u007f,]/.test(input))) { setAgentError('Enter a process name or an absolute local executable path, not a command.'); return; }
    if (draft.processRules.length >= 100) { setAgentError('The agent list is full.'); return; }
    const duplicate = draft.processRules.some(rule => isPath
      ? rule.executablePaths.some(path => pathKey(path) === pathKey(input))
      : !rule.executablePaths.length && (rule.processNames ?? (rule.scriptPathSuffixes.length ? [] : rule.executableBasenames)).some(name => nameKey(name) === nameKey(input)));
    if (duplicate) { setAgentError('That program is already listed.'); return; }
    change(current => ({ ...current, processRules: [...current.processRules, createAgentRule(input)] })); setAgentName(''); setAgentError('');
  }
  function fieldError(field: string) { const message = errors[field] ?? (saveError?.field === field ? saveError.error.message : null); return message ? <small className="field-error" role="alert">{message}</small> : null; }
  return <div className="settings-body">
    <span className={`saved-indicator${saved ? ' visible' : ''}`} role="status" aria-hidden={!saved}>Saved</span>
    <section><h3>Accent color</h3><div className="swatches">{SWATCHES.map(color => <button key={color} className="swatch" style={{ backgroundColor: color }} aria-label={`Use ${color}`} aria-pressed={draft.accentColor.toLowerCase() === color} onClick={() => change(current => ({ ...current, accentColor: color }))} />)}<input type="color" aria-label="Accent color" title="Custom accent color" value={draft.accentColor} onChange={event => change(current => ({ ...current, accentColor: event.target.value }))} /></div>{fieldError('accent')}</section>
    <section><h3>Background color</h3><div className="swatches">{BACKGROUNDS.map(color => <button key={color} className="swatch" style={{ backgroundColor: color }} aria-label={`Use background ${color}`} aria-pressed={draft.backgroundColor.toLowerCase() === color} onClick={() => change(current => ({ ...current, backgroundColor: color }))} />)}<input type="color" aria-label="Background color" title="Custom background color" value={draft.backgroundColor} onChange={event => change(current => ({ ...current, backgroundColor: event.target.value }))} /></div><p className="settings-hint">Text and all other colors adapt automatically.</p>{fieldError('background')}</section>
    <section><h3>Default shell</h3><select aria-label="Default shell" value={draft.terminalProfileId ?? defaultProfileId ?? ''} onChange={event => change(current => ({ ...current, terminalProfileId: event.target.value }))}>{!profiles.length && <option value="">No shells available</option>}{profiles.map(profile => <option key={profile.id} value={profile.id} disabled={!profile.available} title={profile.unavailableReason ?? undefined}>{profile.label}</option>)}</select>{fieldError('shell')}</section>
    {linux && <section><h3>Python</h3><input aria-label="Python path" value={draft.pythonPath ?? ''} placeholder={probe.python?.detected ?? 'Not found'} onChange={event => change(current => ({ ...current, pythonPath: event.target.value.trim() ? event.target.value : null }))} /><p className="settings-hint">Used to safely close terminals. Leave empty for auto-detect.</p>{fieldError('python')}{probe.python?.reason && <small className="settings-hint">{probe.python.reason}</small>}</section>}
    <section><h3>Agents</h3><p className="settings-hint">A tab turns green while one of these programs runs in it. Process names match in any location; executable paths match only that path.</p><div className="agent-list">{draft.processRules.map(rule => {
      const label = BUILTIN_LABELS[rule.id] ?? rule.label;
      return <div className="agent-rule" key={rule.id}><div className="agent-row"><button role="switch" aria-checked={rule.enabled} aria-label={`Track ${label}`} className="toggle" onClick={() => updateRule(rule.id, { enabled: !rule.enabled })}><span /></button><div className="agent-copy"><span>{label}</span>{!BUILTIN_IDS.has(rule.id) && <small>{ruleSummary(rule)}</small>}</div>{!BUILTIN_IDS.has(rule.id) && <button className="icon-button" aria-label={`Remove ${label}`} onClick={() => change(current => ({ ...current, processRules: current.processRules.filter(item => item.id !== rule.id) }))}><Icon name="close" /></button>}</div>{fieldError(`${rule.id}.paths`)}{fieldError(`${rule.id}.suffixes`)}</div>;
    })}</div><form className="add-agent" onSubmit={event => { event.preventDefault(); addAgent(); }}><input aria-label="Add agent" placeholder="Process name or absolute executable path" value={agentName} onChange={event => { setAgentName(event.target.value); setAgentError(''); }} aria-invalid={!!agentError} aria-describedby={agentError ? 'add-agent-error' : undefined} maxLength={32760} /><button type="submit" disabled={!agentName.trim()}>Add</button></form>{agentError && <small id="add-agent-error" className="field-error" role="alert">{agentError}</small>}{fieldError('agents')}</section>
    <section><h3>Explorer</h3><div className="explorer-row"><button className="toggle" role="switch" aria-label={linux ? 'Add Open in Shellfox to the file manager right-click menu' : 'Add Open in Shellfox to Explorer right-click menu'} aria-checked={explorerState.installed} disabled={!explorerState.supported || action.pending} onClick={() => void action.run(() => client.api.setExplorerIntegration({ installed: !explorerState.installed })).then(result => { if (result?.ok) { setExplorerState(result.value); client.refresh(); } })}><span /></button><span>{linux ? 'Add “Open in Shellfox” to the file manager right-click menu' : 'Add “Open in Shellfox” to Explorer right-click menu'}</span></div>{(explorerState.reason || !explorerState.supported) && <small className="settings-hint">{explorerState.reason ?? 'Unavailable on this system.'}</small>}</section>
    <section><h3>Terminal command</h3><div className="explorer-row"><button className="toggle" role="switch" aria-label="Enable shellfox start <path> in terminals" aria-checked={cliState.installed} disabled={!cliState.supported || action.pending || cliState.reason === 'Installed with the package'} onClick={() => void action.run(() => client.api.setCliIntegration({ installed: !cliState.installed })).then(result => { if (result?.ok) { setCliState(result.value); client.refresh(); } })}><span /></button><span>Enable <code>shellfox start &lt;path&gt;</code> in terminals</span></div>{(cliState.reason || !cliState.supported) && <small className="settings-hint">{cliState.reason ?? 'Unavailable on this system.'}</small>}<code className="shellfox-cli-example">shellfox start .</code></section>
  </div>;
}
