import { createStore } from 'zustand/vanilla';
import type { AppError, ManagerApi, ProcessRule, SettingsDto } from '../shared/contracts';
import { request } from './api';

type Edit = { value: unknown; version: number };
type Changes = Map<string, unknown>;
const equal = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const rulePath = (id: string, field?: string) => `rule:${id}${field ? `:${field}` : ''}`;
const localPath = (value: string) => (/^[a-z]:[\\/]/i.test(value) || /^\/(?!\/)/.test(value)) && !value.includes('://') && !/^[\\/]{2}/.test(value) && !/[\u0000-\u001f\u007f]/.test(value);
export function validateSettingsDraft(settings: SettingsDto): Record<string, string> {
  const errors: Record<string, string> = {};
  if (!/^#[0-9a-f]{6}$/i.test(settings.accentColor)) errors.accent = 'Choose a six-digit color.';
  for (const rule of settings.processRules) {
    if (rule.executablePaths.some(path => !localPath(path))) errors[`${rule.id}.paths`] = 'Use absolute local paths, separated by commas.';
    if (rule.scriptPathSuffixes.some(path => path.split(/[\\/]/).some(part => part === '.' || part === '..') || /[\u0000-\u001f\u007f]/.test(path))) errors[`${rule.id}.suffixes`] = 'Use script paths without . or .. components.';
    if (rule.executablePaths.length > 30 || rule.scriptPathSuffixes.length > 30) errors[`${rule.id}.paths`] = 'Use at most 30 entries.';
  }
  return errors;
}

// Rules are merged by id and field, not as one array. Editing a toggle must not
// overwrite an external path update or a newly added rule.
function diff(before: SettingsDto, after: SettingsDto): Changes {
  const changes: Changes = new Map();
  for (const key of new Set([...Object.keys(before), ...Object.keys(after)]) as Set<keyof SettingsDto>) {
    if (key !== 'processRules' && !equal(before[key], after[key])) changes.set(key, after[key]);
  }
  const previous = new Map(before.processRules.map(rule => [rule.id, rule]));
  const next = new Map(after.processRules.map(rule => [rule.id, rule]));
  for (const [id, rule] of previous) {
    const updated = next.get(id);
    if (!updated) { changes.set(rulePath(id), null); continue; }
    for (const field of Object.keys(updated) as (keyof ProcessRule)[]) {
      if (field !== 'id' && !equal(rule[field], updated[field])) changes.set(rulePath(id, field), updated[field]);
    }
  }
  for (const [id, rule] of next) if (!previous.has(id)) changes.set(rulePath(id), rule);
  return changes;
}
function apply(base: SettingsDto, changes: Changes): SettingsDto {
  const next = structuredClone(base);
  // Apply structural edits before individual rule fields, regardless of order.
  for (const [path, value] of changes) {
    if (path.startsWith('rule:')) {
      const [, id, field] = path.split(':');
      if (field) continue;
      next.processRules = next.processRules.filter(rule => rule.id !== id);
      if (value !== null) next.processRules.push(structuredClone(value as ProcessRule));
    } else Object.assign(next, { [path]: structuredClone(value) });
  }
  for (const [path, value] of changes) {
    if (!path.startsWith('rule:')) continue;
    const [, id, field] = path.split(':');
    const rule = next.processRules.find(item => item.id === id);
    if (rule && field) Object.assign(rule, { [field]: structuredClone(value) });
  }
  return next;
}
export interface SettingsState {
  draft: SettingsDto | null;
  dirty: boolean;
  saving: boolean;
  saved: number;
  error: { field: string; error: AppError; toast: boolean } | null;
}

// One controller per manager client. Neither drafts nor the single save queue
// belong to a modal mount. DTOs are assembled only when a request is dispatched.
export function createSettingsController(api: ManagerApi, refresh: () => void) {
  const store = createStore<SettingsState>(() => ({ draft: null, dirty: false, saving: false, saved: 0, error: null }));
  let authoritative: SettingsDto | null = null;
  let observedSignature: string | undefined;
  let snapshotSignature: string | undefined;
  let snapshotRevision = -1;
  const edits = new Map<string, Edit>();
  let version = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let ready = false;
  let saving = false;
  let failedVersion: number | undefined;
  let viewers = 0;
  const values = () => new Map([...edits].map(([path, edit]) => [path, edit.value]));
  function publish() {
    store.setState({ draft: authoritative ? apply(authoritative, values()) : null, dirty: edits.size > 0 });
  }
  function observe(settings: SettingsDto, revision?: number) {
    const signature = JSON.stringify(settings);
    if (revision !== undefined) {
      // A repeated pre-save snapshot cannot undo canonical response fields.
      // A genuinely newer revision may intentionally restore an earlier value.
      if (revision < snapshotRevision || (revision === snapshotRevision && signature === snapshotSignature)) return;
      snapshotRevision = revision; snapshotSignature = signature;
    } else if (signature === observedSignature) return;
    observedSignature = signature;
    authoritative = structuredClone(settings);
    publish();
  }
  function update(transform: (draft: SettingsDto) => SettingsDto) {
    const current = store.getState().draft;
    if (!current) return;
    const changes = diff(current, transform(structuredClone(current)));
    if (!changes.size) return;
    for (const [path, value] of changes) {
      if (path.startsWith('rule:') && path.split(':').length === 2 && value === null) {
        for (const key of edits.keys()) if (key.startsWith(`${path}:`)) edits.delete(key);
      }
      edits.set(path, { value: structuredClone(value), version: ++version });
    }
    store.setState({ error: null });
    publish();
    clearTimeout(timer);
    ready = false;
    timer = setTimeout(() => { timer = undefined; ready = true; void pump(); }, 400);
  }
  async function pump() {
    if (saving) return;
    while (ready && authoritative && edits.size) {
      ready = false;
      const value = apply(authoritative, values());
      if (failedVersion === version) return;
      if (Object.keys(validateSettingsDraft(value)).length) return;
      const dispatchedBase = structuredClone(authoritative);
      const dispatched = new Map(edits);
      const dispatchedVersion = version;
      saving = true; store.setState({ saving: true });
      const result = await request(() => api.saveSettings(value));
      saving = false;
      if (result.ok) {
        failedVersion = undefined;
        // Keep authoritative fields changed by external snapshots during this
        // request, unless this request owned them. Shell normalization belongs
        // to a submitted profile change and must come from the canonical DTO.
        const external = diff(dispatchedBase, authoritative);
        for (const path of external.keys()) {
          if (dispatched.has(path) || (dispatched.has('terminalProfileId') && ['shellId', 'shellExecutable'].includes(path))) external.delete(path);
        }
        authoritative = apply(result.value, external);
        observedSignature = JSON.stringify(authoritative);
        for (const [path, sent] of dispatched) if (edits.get(path)?.version === sent.version) edits.delete(path);
        publish();
        store.setState({ saving: false, error: null, saved: edits.size ? store.getState().saved : store.getState().saved + 1 });
        refresh();
      } else {
        failedVersion = dispatchedVersion;
        const field = dispatched.has('terminalProfileId') ? 'shell' : dispatched.has('accentColor') ? 'accent' : 'agents';
        store.setState({ saving: false, error: { field, error: result.error, toast: viewers === 0 } });
        // Do not retry a failed intent automatically. A genuinely newer edit
        // may still be queued, and will include the retained unsaved fields.
        if (![...edits].some(([path, edit]) => dispatched.get(path)?.version !== edit.version)) ready = false;
      }
    }
  }
  function flush() {
    clearTimeout(timer); timer = undefined; ready = true; void pump();
  }
  return {
    store, observe, update, flush,
    mount() { viewers++; return () => { viewers--; flush(); }; },
  };
}
