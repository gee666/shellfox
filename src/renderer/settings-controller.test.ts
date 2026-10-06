import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { failure, success } from '../shared/contracts';
import { createManagerClient } from './store';
import { createAgentRule } from './SettingsPanel';
import { deferred, mockApi, snapshot } from './test-fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });
function setup() {
  const initial = snapshot(); const fixture = mockApi(initial);
  const client = createManagerClient(fixture.api); const controller = client.settings;
  controller.observe(initial.settings);
  return { initial, fixture, client, controller };
}
const tick = () => vi.advanceTimersByTimeAsync(400);

describe('client-owned settings controller', () => {
  it('coalesces the old modal flush and newer modal edits into one ordered save', async () => {
    const { initial, fixture, controller } = setup();
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.saveSettings>>>();
    fixture.api.saveSettings.mockReturnValueOnce(pending.promise);
    const closeFirst = controller.mount();
    controller.update(draft => ({ ...draft, accentColor: '#60a5fa' }));
    await tick(); expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1);
    controller.update(draft => ({ ...draft, processRules: draft.processRules.map(rule => ({ ...rule, enabled: false })) }));
    closeFirst();
    const closeSecond = controller.mount();
    controller.observe(initial.settings);
    expect(controller.store.getState().draft?.accentColor).toBe('#60a5fa');
    expect(controller.store.getState().draft?.processRules[0]!.enabled).toBe(false);
    controller.update(draft => ({ ...draft, accentColor: '#4ade80' }));
    await tick(); expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1);
    pending.resolve(success({ ...fixture.api.saveSettings.mock.calls[0]![0], shellExecutable: 'C:\\canonical\\pwsh.exe' }));
    await vi.advanceTimersByTimeAsync(0);
    expect(fixture.api.saveSettings).toHaveBeenCalledTimes(2);
    expect(fixture.api.saveSettings.mock.calls[1]![0]).toMatchObject({ accentColor: '#4ade80', shellExecutable: 'C:\\canonical\\pwsh.exe' });
    expect(fixture.api.saveSettings.mock.calls[1]![0].processRules[0]!.enabled).toBe(false);
    closeSecond(); await tick(); expect(fixture.api.saveSettings).toHaveBeenCalledTimes(2);
  });

  it('merges external hidden fields into a dirty draft before dispatch', async () => {
    const { initial, fixture, controller } = setup();
    controller.update(draft => ({ ...draft, accentColor: '#60a5fa' }));
    controller.observe({ ...initial.settings, historyPageSize: 3, shellExecutable: 'C:\\external\\pwsh.exe' });
    await tick();
    expect(fixture.api.saveSettings.mock.calls[0]![0]).toMatchObject({ accentColor: '#60a5fa', historyPageSize: 3, shellExecutable: 'C:\\external\\pwsh.exe' });
  });

  it('rebases canonical fields while preserving later profile and rule edits', async () => {
    const { fixture, controller } = setup();
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.saveSettings>>>();
    fixture.api.saveSettings.mockReturnValueOnce(pending.promise);
    controller.update(draft => ({ ...draft, terminalProfileId: 'wsl:Ubuntu' }));
    await tick();
    controller.update(draft => ({ ...draft, terminalProfileId: 'pwsh', processRules: draft.processRules.map(rule => ({ ...rule, enabled: false })) }));
    await tick();
    pending.resolve(success({ ...fixture.api.saveSettings.mock.calls[0]![0], shellId: 'wsl', shellExecutable: 'C:\\Windows\\System32\\wsl.exe' }));
    await vi.advanceTimersByTimeAsync(0);
    const second = fixture.api.saveSettings.mock.calls[1]![0];
    expect(second).toMatchObject({ terminalProfileId: 'pwsh', shellId: 'wsl', shellExecutable: 'C:\\Windows\\System32\\wsl.exe' });
    expect(second.processRules[0]!.enabled).toBe(false);
    expect(controller.store.getState().draft).toEqual(second);
  });

  it('does not roll back a newer untouched snapshot field when a delayed response arrives', async () => {
    const { initial, fixture, controller } = setup();
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.saveSettings>>>();
    fixture.api.saveSettings.mockReturnValueOnce(pending.promise);
    controller.update(draft => ({ ...draft, accentColor: '#60a5fa' })); await tick();
    controller.observe({ ...initial.settings, historyPageSize: 3 });
    controller.update(draft => ({ ...draft, processRules: draft.processRules.map(rule => ({ ...rule, enabled: false })) })); await tick();
    pending.resolve(success(fixture.api.saveSettings.mock.calls[0]![0])); await vi.advanceTimersByTimeAsync(0);
    expect(fixture.api.saveSettings.mock.calls[1]![0].historyPageSize).toBe(3);
    expect(controller.store.getState().draft?.historyPageSize).toBe(3);
  });

  it('merges rules by id and field while preserving external additions and paths', async () => {
    const { initial, fixture, controller } = setup();
    const id = initial.settings.processRules[0]!.id;
    controller.update(draft => ({ ...draft, processRules: draft.processRules.map(rule => ({ ...rule, enabled: false })) }));
    const external = createAgentRule('external');
    controller.observe({ ...initial.settings, processRules: [{ ...initial.settings.processRules[0]!, executablePaths: ['C:\\new\\pi.exe'] }, external] });
    await tick();
    const rules = fixture.api.saveSettings.mock.calls[0]![0].processRules;
    expect(rules.find(rule => rule.id === id)).toMatchObject({ enabled: false, executablePaths: ['C:\\new\\pi.exe'] });
    expect(rules.find(rule => rule.id === external.id)).toEqual(external);
  });

  it('ignores a repeated old snapshot after normalization but accepts a newer external reset', async () => {
    const { initial, fixture, controller } = setup();
    controller.observe(initial.settings, 1);
    fixture.api.saveSettings.mockImplementationOnce(async value => success({ ...value, shellExecutable: 'C:\\canonical\\pwsh.exe' }));
    controller.update(draft => ({ ...draft, accentColor: '#60a5fa' })); await tick();
    controller.observe(initial.settings, 1);
    expect(controller.store.getState().draft?.shellExecutable).toBe('C:\\canonical\\pwsh.exe');
    controller.observe(initial.settings, 2);
    expect(controller.store.getState().draft?.shellExecutable).toBeNull();
    expect(controller.store.getState().draft?.accentColor).toBe(initial.settings.accentColor);
  });

  it('persists a close-time failure for toast presentation without retrying the same intent', async () => {
    const { fixture, controller } = setup();
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.saveSettings>>>();
    fixture.api.saveSettings.mockReturnValueOnce(pending.promise);
    const close = controller.mount();
    controller.update(draft => ({ ...draft, accentColor: '#60a5fa' })); close();
    pending.resolve(failure('STORAGE_FAILED', 'Cannot save.')); await vi.advanceTimersByTimeAsync(0);
    expect(controller.store.getState().error).toMatchObject({ toast: true, error: { message: 'Cannot save.' } });
    controller.flush(); await tick(); expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1);
    controller.update(draft => ({ ...draft, accentColor: '#4ade80' })); await tick();
    expect(fixture.api.saveSettings).toHaveBeenCalledTimes(2);
    expect(controller.store.getState().error).toBeNull();
  });
});
