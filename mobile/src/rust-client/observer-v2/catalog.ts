import { z } from 'zod';
import { adapterSchema, shellIdSchema, pathSchema, titleSchema, lifecycleSchema,
  statusSchema, tabStatusSchema, sessionWindowSchema,
  errorSchema, profileIdSchema, probeSchema, explorerSchema,
  cliIntegrationSchema,
} from '../../../../src/shared/schemas';
import { observerSettingsSchema, observerEnvironment } from './values';
import { tabStatSchema } from '../../../../src/shared/process-stats-schemas';
import { fact, observerId, count, integer, timestamp, sequence, boundedText, projectionIssueSchema } from './primitives';

export const tabSchema = z.object({
  id: observerId, sessionId: observerId, title: titleSchema,
  sourceKind: fact(z.enum(['pty', 'automationPipe', 'externalBytes'])),
  facts: z.object({
    cwd: fact(pathSchema), ordinal: fact(count), createdAt: fact(timestamp),
    lifecycle: fact(lifecycleSchema), status: fact(tabStatusSchema), agents: fact(count),
    monitoringReason: fact(boundedText(1000).nullable()), error: fact(errorSchema.nullable()),
    generation: fact(observerId.nullable()), terminalKind: fact(z.enum(['embedded', 'external-legacy']).nullable()),
    profileId: fact(profileIdSchema.nullable()), exitCode: fact(integer.nullable()),
  }).strict(),
}).strict();
export const sessionSchema = z.object({
  id: observerId, title: titleSchema,
  facts: z.object({
    cwd: fact(pathSchema), adapterId: fact(adapterSchema), shellId: fact(shellIdSchema),
    createdAt: fact(timestamp), updatedAt: fact(timestamp), settledAt: fact(timestamp.nullable()),
    pinnedAt: fact(timestamp.nullable()), status: fact(statusSchema), activityStatus: fact(tabStatusSchema),
    counts: fact(z.object({ running: count, waiting: count, unknown: count, error: count, closed: count, agents: count }).strict()),
    env: fact(observerEnvironment(true)), window: fact(sessionWindowSchema.nullable()),
    canFocus: fact(z.boolean()), canAddTab: fact(z.boolean()), controlReason: fact(boundedText(1000).nullable()),
    error: fact(errorSchema.nullable()), terminalLifetime: fact(z.enum(['app-owned', 'external-legacy']).nullable()),
    shellSurvival: fact(z.boolean().nullable()),
  }).strict(),
  tabs: z.array(tabSchema).max(1000),
}).strict().refine(value => value.tabs.every(tab => tab.sessionId === value.id)
  && new Set(value.tabs.map(tab => tab.id)).size === value.tabs.length);
export const catalogEntrySchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('session'), value: sessionSchema }).strict(),
  z.object({ kind: z.literal('unavailable'), sessionId: observerId, issues: z.array(projectionIssueSchema).min(1).max(64) }).strict(),
]);
export const snapshotSchema = z.object({
  revision: sequence, coverage: z.object({ mode: z.literal('complete'), entryCount: z.number().int().min(0).max(10000) }).strict(),
  entries: z.array(catalogEntrySchema).max(10000), settings: fact(observerSettingsSchema),
  probe: fact(probeSchema), explorer: fact(explorerSchema), cli: fact(cliIntegrationSchema),
}).strict().refine(value => {
  if (value.coverage.entryCount !== value.entries.length) return false;
  const sessions = new Set<string>(), tabs = new Set<string>();
  for (const entry of value.entries) {
    const id = entry.kind === 'session' ? entry.value.id : entry.sessionId;
    if (sessions.has(id)) return false;
    sessions.add(id);
    if (entry.kind === 'session') for (const tab of entry.value.tabs) {
      if (tabs.has(tab.id)) return false;
      tabs.add(tab.id);
    }
  }
  return true;
});
export const sessionStatsSchema = z.object({
  sessions: z.array(sessionSchema).max(10000), tabs: z.array(tabStatSchema).max(64),
}).strict().refine(value => new Set(value.sessions.map(session => session.id)).size === value.sessions.length
  && value.sessions.every(session => session.facts.env.state !== 'known' || session.facts.env.value.length === 0));
export type ObserverSnapshot = z.infer<typeof snapshotSchema>;
export type ObserverCatalogEntry = z.infer<typeof catalogEntrySchema>;
export type ObserverSession = z.infer<typeof sessionSchema>;
export type ObserverTab = z.infer<typeof tabSchema>;
