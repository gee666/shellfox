import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import {
  sessionSchema, settingsSchema, snapshotSchema, terminalAttachmentSchema,
} from '../../../src/shared/schemas';
import { remoteHostEventSchema } from '../../../src/shared/remote-schemas';
import { aggregateStatus, countTabs, publicStatus } from '../../../src/shared/status';

const fixturePath = process.env.SHELLFOX_ADAPTER_FIXTURES;
if (!fixturePath) throw new Error('Run node crates/shellfox-remote/tests/run-dto-fixtures.mjs');
const raw: unknown = JSON.parse(readFileSync(fixturePath, 'utf8'));
const fixtures = z.object({
  snapshots: z.array(snapshotSchema).length(6),
  settings: z.array(settingsSchema).length(6),
  persistedSettings: z.array(settingsSchema).length(3),
  attachments: z.array(terminalAttachmentSchema).length(3),
  events: z.array(remoteHostEventSchema).length(8),
  statusSessions: z.array(sessionSchema).length(25),
}).strict().parse(raw);

describe('Rust adapter output against actual shared Zod schemas', () => {
  it('preserves actual core settings omission/null/opaque profile semantics', () => {
    const [omitted, cleared, opaque] = fixtures.persistedSettings;
    expect(omitted).not.toHaveProperty('terminalProfileId');
    expect(cleared.terminalProfileId).toBeNull();
    expect(opaque.terminalProfileId).toBe('opaque-imported-profile');
    expect(opaque.processRules.length).toBeGreaterThan(0);
    expect(opaque.version).toBe(1);
  });
  it.each(fixtures.snapshots)('accepts complete $probe.platform manager output', snapshot => {
    expect(snapshot.sessions[0].status).toBe('unknown');
    expect(snapshot.sessions[0].counts.running).toBe(0);
    expect(snapshot.sessions[0].tabs[0].monitoringReason).toBe('Process monitor unavailable');
    expect(snapshot.settings.processRules[0].id).toBe('custom-agent-1');
    expect(snapshot.settings.terminalEnv).toEqual([{ name: 'GLOBAL', value: 'kept' }]);
    expect(snapshot.settings.accentColor).toBe('#123456');
    expect(snapshot.settings.autoUpdate).toBe(false);
    expect(snapshot.explorer.supported).toBe(false);
    expect(snapshot.cli.installed).toBe(false);
    expect(snapshot.probe.available).toBe(false);
    expect(snapshot.sessions[0].env).toEqual([{ name: 'SAVED', value: 'saved, not inherited' }]);
  });

  it('matches real shared status/count rules for all 25 mixed-state pairs', () => {
    for (const session of fixtures.statusSessions) {
      expect(session.counts).toEqual(countTabs(session.tabs));
      expect(session.activityStatus).toBe(aggregateStatus(session.tabs, session.error));
      expect(session.status).toBe(publicStatus(session.activityStatus, session.settledAt));
    }
  });

  it('contains owned replay, ANSI recovery and blocked attachments with exact fencing', () => {
    const [replay, recovery, blocked] = fixtures.attachments;
    expect(replay.control).toBe('owned');
    expect(recovery.control).toBe('owned');
    expect(blocked.control).toBe('blocked');
    if (replay.control !== 'owned' || recovery.control !== 'owned') throw new Error('Expected owned fixtures');
    expect(replay.snapshot).toBe(false);
    expect(recovery.snapshot).toBe(true);
    expect(recovery.state).toBe('closed');
    expect(recovery.exitCode).toBe(0);
    expect(replay.chunks[1].data).toBe('世界\r\n');
    expect(replay.settledAt).toBeNull();
    expect(replay.controlEpoch).toBe(3);
    expect(blocked).not.toHaveProperty('chunks');
  });

  it('covers all six terminal events and both metadata change reasons', () => {
    expect(fixtures.events.filter(event => event.type === 'terminal').map(event => event.event.type))
      .toEqual(['settlement', 'activity', 'data', 'exit', 'error', 'control']);
    expect(fixtures.events.filter(event => event.type === 'changed').map(event => event.event.reason))
      .toEqual(['sessions', 'settings']);
  });

  it('rejects non-contract manager DTOs rather than accepting a small core snapshot', () => {
    expect(snapshotSchema.safeParse({ revision: 1, sessions: [] }).success).toBe(false);
    expect(snapshotSchema.safeParse({ ...fixtures.snapshots[0], selection: {} }).success).toBe(false);
    expect(snapshotSchema.safeParse({
      ...fixtures.snapshots[0], settings: { ...fixtures.settings[0], fontSize: 14 },
    }).success).toBe(false);
  });

  it('rejects wrong chunk generation, unsafe epochs and oversized UTF-8', () => {
    const replay = fixtures.attachments[0];
    if (replay.control !== 'owned') throw new Error('Expected owned fixture');
    expect(terminalAttachmentSchema.safeParse({ ...replay, controlEpoch: 0 }).success).toBe(false);
    expect(terminalAttachmentSchema.safeParse({ ...replay, controlEpoch: Number.MAX_SAFE_INTEGER + 1 }).success).toBe(false);
    expect(terminalAttachmentSchema.safeParse({ ...replay, chunks: [
      { ...replay.chunks[0], generation: '44444444-4444-4444-8444-444444444444' },
    ] }).success).toBe(false);
    expect(terminalAttachmentSchema.safeParse({ ...replay, chunks: [
      { ...replay.chunks[0], data: '世'.repeat(6000) },
    ] }).success).toBe(false);
  });
});
