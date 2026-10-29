import { useEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import type { ProcessRule, SettingsDto, TerminalProfileDto } from '../shared/contracts';
import type { ManagerClient } from './store';
import { executablePathSchema, validateSettingsDraft } from './settings-controller';
import { Icon } from './components';
import { ColorSwatches } from './ColorSwatches';
import { SshConnections } from './SshConnections';
import { RemoteSettings } from './RemoteSettings';
import type { RemoteController } from './remote-controller';
import './settings.css';

const TABS = ['General', 'Terminals', 'Agents', 'Remote access', 'SSH'] as const;
export type SettingsTab = typeof TABS[number];
function PendingClientsCount({ remote }: { remote: RemoteController }) {
  const count = useStore(remote.store, state => state.state?.pending.length ?? 0);
  return count ? <span className="settings-tab-count" aria-hidden="true" title={`${count} pending clients`}>{count}</span> : null;
}

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
export function SettingsPanel({ client, settings, profiles, defaultProfileId, onClose, remote, activeTab, onTabChange }: {
  client: ManagerClient; settings: SettingsDto; profiles: TerminalProfileDto[]; defaultProfileId: string | null; onClose?: () => void; remote?: RemoteController; activeTab?: SettingsTab; onTabChange?: (tab: SettingsTab) => void;
}) {
  const controller = client.settings;
  const state = useStore(controller.store);
  const draft = state.draft ?? settings;
  const [saved, setSaved] = useState(false);
  const [localTab, setLocalTab] = useState<SettingsTab>('General');
  const tab = activeTab ?? localTab;
  function setTab(next: SettingsTab) { setLocalTab(next); onTabChange?.(next); }
  const tabs = useRef<HTMLDivElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  useEffect(() => { closeButton.current?.focus(); }, []);
  const saveError = state.error;
  const previousProps = useRef(settings);
  useEffect(() => {
    // Reopening uses the controller's latest draft, not the last page props.
    // Standalone panels can still receive authoritative updates through props.
    if (!controller.store.getState().draft || previousProps.current !== settings) controller.observe(settings);
    previousProps.current = settings;
  }, [controller, settings]);
  useEffect(() => controller.mount(), [controller]);
  const [agentName, setAgentName] = useState('');
  const [agentError, setAgentError] = useState('');
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
  return <div className="settings-page" role="region" aria-label="Settings">
    <header className="settings-header"><h2>Settings <small>This device</small></h2><div><span className={`saved-indicator${saved || state.saving || state.dirty ? ' visible' : ''}`} role="status" aria-hidden={!saved && !state.saving && !state.dirty}>{state.saving ? 'Saving…' : state.dirty ? saveError || Object.keys(errors).length ? 'Check settings' : 'Unsaved changes' : 'Saved'}</span>{onClose && <button ref={closeButton} className="icon-button" aria-label="Close settings" title="Close settings (Esc)" onClick={onClose}><Icon name="close" /></button>}</div></header>
    <div ref={tabs} className="settings-tabs" role="tablist" aria-label="Settings categories" onKeyDown={event => {
      const index = TABS.indexOf(tab);
      const next = event.key === 'ArrowRight' ? (index + 1) % TABS.length : event.key === 'ArrowLeft' ? (index + TABS.length - 1) % TABS.length : event.key === 'Home' ? 0 : event.key === 'End' ? TABS.length - 1 : null;
      if (next === null) return;
      event.preventDefault(); setTab(TABS[next]!); tabs.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
    }}>{TABS.map(name => {
      const fields = name === 'General' ? ['accent', 'background', 'historyPageSize', 'sessionSettleHours', 'sessionDeleteHours'] : name === 'Terminals' ? ['shell', 'python'] : name === 'Agents' ? ['agents', ...Object.keys(errors).filter(key => key.includes('.'))] : [];
      const invalid = fields.some(field => errors[field] || saveError?.field === field);
      const id = name.replaceAll(' ', '-');
      return <button key={name} role="tab" aria-label={invalid ? `${name}, needs attention` : undefined} id={`settings-tab-${id}`} aria-controls={`settings-panel-${id}`} aria-selected={tab === name} tabIndex={tab === name ? 0 : -1} onClick={() => setTab(name)}>{name}{name === 'Remote access' && remote && <PendingClientsCount remote={remote} />}{invalid && <span className="settings-tab-error" aria-hidden="true">•</span>}</button>;
    })}</div>
    <div className="settings-scroll"><div className="settings-body">
    <div role="tabpanel" id="settings-panel-General" aria-labelledby="settings-tab-General" hidden={tab !== 'General'} inert={tab !== 'General'}>
    <section><h3>Updates</h3><label><input type="checkbox" checked={draft.autoUpdate} onChange={event => change(current => ({ ...current, autoUpdate: event.target.checked }))} /> Automatic updates</label><p className="settings-hint">Download new versions automatically and install when you quit. Turn off to update manually.</p></section>
    <section><h3>Accent color</h3><ColorSwatches kind="accent" presets={SWATCHES} value={draft.accentColor} onChange={color => change(current => ({ ...current, accentColor: color }))} />{fieldError('accent')}</section>
    <section><h3>Background color</h3><ColorSwatches kind="background" presets={BACKGROUNDS} value={draft.backgroundColor} onChange={color => change(current => ({ ...current, backgroundColor: color }))} /><p className="settings-hint">Text and all other colors adapt automatically.</p>{fieldError('background')}</section>
    <section className="session-settings"><h3>Session history</h3>
      <label>Sessions per page<input type="number" min={1} max={100} step={1} value={Number.isFinite(draft.historyPageSize) ? draft.historyPageSize : ''} onChange={event => change(current => ({ ...current, historyPageSize: event.target.valueAsNumber }))} /></label>{fieldError('historyPageSize')}
      <label>Settle after (hours)<input type="number" min={1} max={87600} step="any" value={Number.isFinite(draft.sessionSettleHours) ? draft.sessionSettleHours : ''} onChange={event => change(current => ({ ...current, sessionSettleHours: event.target.valueAsNumber }))} /></label>{fieldError('sessionSettleHours')}
      <label>Delete settled after (hours)<input type="number" min={1} max={87600} step="any" value={Number.isFinite(draft.sessionDeleteHours) ? draft.sessionDeleteHours : ''} onChange={event => change(current => ({ ...current, sessionDeleteHours: event.target.valueAsNumber }))} /></label>{fieldError('sessionDeleteHours')}
    </section>
    </div><div role="tabpanel" id="settings-panel-Terminals" aria-labelledby="settings-tab-Terminals" hidden={tab !== 'Terminals'} inert={tab !== 'Terminals'}>
    <section><h3>Default shell</h3><select aria-label="Default shell" value={draft.terminalProfileId ?? defaultProfileId ?? ''} onChange={event => change(current => ({ ...current, terminalProfileId: event.target.value }))}>{!profiles.length && <option value="">No shells available</option>}{profiles.filter(profile => !profile.elevation).map(profile => <option key={profile.id} value={profile.id} disabled={!profile.available} title={profile.unavailableReason ?? undefined}>{profile.label}</option>)}</select><p className="settings-hint settings-control-note">Used for new local terminals.</p>{fieldError('shell')}</section>
    {linux && <section><h3>Python</h3><input aria-label="Python path" value={draft.pythonPath ?? ''} placeholder={probe.python?.detected ?? 'Not found'} onChange={event => change(current => ({ ...current, pythonPath: event.target.value.trim() ? event.target.value : null }))} /><p className="settings-hint">Used to safely close terminals. Leave empty for auto-detect.</p>{fieldError('python')}{probe.python?.reason && <small className="settings-hint">{probe.python.reason}</small>}</section>}
    </div><div role="tabpanel" id="settings-panel-Agents" aria-labelledby="settings-tab-Agents" hidden={tab !== 'Agents'} inert={tab !== 'Agents'}>
    <section><h3>Tracked programs</h3><p className="settings-hint">Tabs turn green while these programs run. Use a process name or an exact executable path.</p><div className="agent-list">{draft.processRules.map(rule => {
      const label = BUILTIN_LABELS[rule.id] ?? rule.label;
      return <div className="agent-rule" key={rule.id}><div className="agent-row"><button role="switch" aria-checked={rule.enabled} aria-label={`Track ${label}`} className="toggle" onClick={() => updateRule(rule.id, { enabled: !rule.enabled })}><span /></button><div className="agent-copy"><span>{label}</span>{!BUILTIN_IDS.has(rule.id) && <small>{ruleSummary(rule)}</small>}</div>{!BUILTIN_IDS.has(rule.id) && <button className="icon-button" aria-label={`Remove ${label}`} onClick={() => change(current => ({ ...current, processRules: current.processRules.filter(item => item.id !== rule.id) }))}><Icon name="close" /></button>}</div>{fieldError(`${rule.id}.paths`)}{fieldError(`${rule.id}.suffixes`)}</div>;
    })}</div><form className="add-agent" onSubmit={event => { event.preventDefault(); addAgent(); }}><input aria-label="Add agent" placeholder="Process name or absolute executable path" value={agentName} onChange={event => { setAgentName(event.target.value); setAgentError(''); }} aria-invalid={!!agentError} aria-describedby={agentError ? 'add-agent-error' : undefined} maxLength={32760} /><button type="submit" disabled={!agentName.trim()}>Add</button></form>{agentError && <small id="add-agent-error" className="field-error" role="alert">{agentError}</small>}{fieldError('agents')}</section>
    </div><div role="tabpanel" id="settings-panel-Remote-access" aria-labelledby="settings-tab-Remote-access" hidden={tab !== 'Remote access'} inert={tab !== 'Remote access'}>
    {remote?.api && <RemoteSettings controller={remote} local={client.api} />}
    </div><div role="tabpanel" id="settings-panel-SSH" aria-labelledby="settings-tab-SSH" hidden={tab !== 'SSH'} inert={tab !== 'SSH'}>
    <SshConnections api={client.api} />
    </div></div></div>
  </div>;
}
