import { it, expect } from 'vitest';
import { randomUUID } from 'node:crypto';
import { remoteMethodSchema } from './remote-schemas';
import { requestSchemas, terminalBackendEventSchema, terminalEventSchema, terminalAttachmentSchema, terminalProfileSchema } from './schemas';
it('allows only declared administrator elevation, not arbitrary runas options', () => {
  const profile = { id: 'administrator-powershell', label: 'Administrator PowerShell', environment: 'local', executable: 'C:\\Windows\\powershell.exe', args: ['-NoLogo'], distro: null, available: true, elevation: 'administrator' };
  expect(terminalProfileSchema.safeParse(profile).success).toBe(true);
  expect(terminalProfileSchema.safeParse({ ...profile, elevation: 'system' }).success).toBe(false);
  expect(terminalProfileSchema.safeParse({ ...profile, runas: 'someone' }).success).toBe(false);
  expect(requestSchemas.addTab.safeParse({ sessionId: randomUUID(), profileId: profile.id, elevation: true }).success).toBe(false);
});
const identity = () => ({ tabId: randomUUID(), generation: randomUUID() });
it('strictly validates every terminal control method and rejects arbitrary launch templates', () => {
  const id = identity(), owned = { ...id, controlEpoch: 1 };
  for (const [method, payload] of [
    ['writeTerminal', { ...owned, data: 'ls\r' }], ['resizeTerminal', { ...owned, cols: 80, rows: 24 }], ['closeTab', id],
    ['attachTerminal', { tabId: id.tabId, afterSequence: 0 }], ['acknowledgeTerminal', { ...owned, sequence: 0 }],
    ['detachTerminal', id], ['takeTerminalControl', id], ['getTerminalProfiles', {}],
  ] as const) {
    expect(requestSchemas[method].safeParse(payload).success).toBe(true);
    expect(requestSchemas[method].safeParse({ ...payload, command: 'arbitrary' }).success).toBe(false);
  }
  expect(remoteMethodSchema.safeParse('takeTerminalControl').success).toBe(true);
  expect(remoteMethodSchema.safeParse('setTerminalVisibility').success).toBe(false);
  expect(requestSchemas.addTab.safeParse({ sessionId: randomUUID(), profileId: 'wsl:Ubuntu', cwd: '/home/user' }).success).toBe(true);
  expect(requestSchemas.addTab.safeParse({ sessionId: randomUUID(), executable: '/evil', args: [] }).success).toBe(false);
});
it('requires an exact positive control epoch for input, size and acknowledgements', () => {
  const id = identity();
  for (const [method, payload] of [
    ['writeTerminal', { ...id, data: 'x' }], ['resizeTerminal', { ...id, cols: 80, rows: 24 }], ['acknowledgeTerminal', { ...id, sequence: 1 }],
  ] as const) {
    expect(requestSchemas[method].safeParse(payload).success).toBe(false);
    for (const controlEpoch of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) expect(requestSchemas[method].safeParse({ ...payload, controlEpoch }).success).toBe(false);
    expect(requestSchemas[method].safeParse({ ...payload, controlEpoch: 1 }).success).toBe(true);
  }
});
it('bounds bytes, accepts literal VT output, and separates raw backend events from routed frames', () => {
  const id = identity(), controlled = { ...id, controlEpoch: 1 };
  expect(requestSchemas.writeTerminal.safeParse({ ...controlled, data: '雪'.repeat(22000) }).success).toBe(false);
  expect(requestSchemas.writeTerminal.safeParse({ ...controlled, data: '\0' }).success).toBe(false);
  const raw = { type: 'data', ...id, sequence: 1, data: '\x1b[31m\0\r\n' };
  expect(terminalBackendEventSchema.safeParse(raw).success).toBe(true);
  expect(terminalEventSchema.safeParse(raw).success).toBe(false);
  expect(terminalEventSchema.safeParse({ ...raw, controlEpoch: 1 }).success).toBe(true);
  expect(terminalEventSchema.safeParse({ ...raw, controlEpoch: 1, data: '雪'.repeat(5500) }).success).toBe(false);
});
it('rejects duplicate/out-of-order replay, mixed identities and dishonest lifetime declarations', () => {
  const id = identity(), chunk = { type: 'data' as const, ...id, sequence: 1, data: 'x' };
  const attachment = { ...id, control: 'owned', controlEpoch: 1, sessionId: randomUUID(), firstSequence: 1, lastSequence: 1, chunks: [chunk], truncated: false, state: 'open', exitCode: null, cols: 80, rows: 24, lifetime: 'app-owned' };
  expect(terminalAttachmentSchema.safeParse(attachment).success).toBe(true);
  expect(terminalAttachmentSchema.safeParse({ ...attachment, chunks: [chunk, chunk] }).success).toBe(false);
  expect(terminalAttachmentSchema.safeParse({ ...attachment, chunks: [{ ...chunk, generation: randomUUID() }] }).success).toBe(false);
  expect(terminalAttachmentSchema.safeParse({ ...attachment, lifetime: 'detached' }).success).toBe(false);
  expect(terminalAttachmentSchema.safeParse({ ...attachment, chunks: Array.from({ length: 70 }, (_, i) => ({ ...chunk, sequence: i + 1, data: 'x'.repeat(4096) })), lastSequence: 70 }).success).toBe(false);
});
it('blocked attachments and ownership events cannot carry terminal contents', () => {
  const id = identity(), blocked = { ...id, sessionId: randomUUID(), control: 'blocked', controlEpoch: 2 };
  expect(terminalAttachmentSchema.safeParse(blocked).success).toBe(true);
  for (const extra of [{ chunks: [] }, { data: 'private' }, { cols: 80 }, { cwd: '/private' }, { snapshot: true }]) {
    expect(terminalAttachmentSchema.safeParse({ ...blocked, ...extra }).success).toBe(false);
  }
  const event = { type: 'control', ...id, control: 'blocked', controlEpoch: 2 };
  expect(terminalEventSchema.safeParse(event).success).toBe(true);
  expect(terminalEventSchema.safeParse({ ...event, data: 'private' }).success).toBe(false);
});
