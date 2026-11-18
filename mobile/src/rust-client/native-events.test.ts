import { describe, expect, it } from 'vitest';
import { nativeOperationSchema, nativeTerminalStateSchema } from './native-events';

const event = {
  operationId: '10', internal: true, status: 0, result: { ok: true, value: { events: [] } },
  connection: { status: 'connected', permission: 'approved', connectionId: '1', retryAfterMs: null },
};
describe('coarse native event contract', () => {
  it('accepts actual owner-only recovery metadata from Rust without performing JS resets', () => {
    const value = { ...event, resetTerminals: ['100'], resetTickets: { '100': '3' },
      resetGeometry: { '100': { cols: 200, rows: 80 } }, recoveryReady: ['101'] };
    expect(nativeOperationSchema.parse(value)).toEqual(value);
    // Android may consume/remove native-only instructions before presentation.
    expect(nativeOperationSchema.parse(event)).toEqual(event);
  });
  it('accepts only known native readiness states with a lossless terminal handle', () => {
    for (const state of ['bound', 'ready', 'recovering', 'closed']) {
      expect(nativeTerminalStateSchema.safeParse({ terminalHandle: '100', state }).success).toBe(true);
    }
    for (const value of [{ terminalHandle: '100', state: 'invalid' },
      { terminalHandle: '100', state: 'ready', rawWriteAllowed: true },
      { terminalHandle: 'pointer:0x123', state: 'ready' },
      { terminalHandle: '100' },
    ]) expect(nativeTerminalStateSchema.safeParse(value).success).toBe(false);
  });
  it('keeps origin required and rejects unknown fields or invalid geometry', () => {
    const { internal: _internal, ...missingOrigin } = event;
    for (const invalid of [missingOrigin, { ...event, terminalOutput: 'not a coarse event' },
      { ...event, resetGeometry: { '100': { cols: 0, rows: 24 } } },
      { ...event, resetGeometry: { '100': { cols: 500, rows: 500 } } },
      { ...event, recoveryReady: ['pointer:0x123'] },
    ]) expect(nativeOperationSchema.safeParse(invalid).success).toBe(false);
  });
});
