import { z } from 'zod';
export const idSchema = z.string().uuid();
export const adapterSchema = z.enum(['windows-terminal', 'gnome-terminal', 'embedded-pty']);
export const shellIdSchema = z.enum(['pwsh', 'windows-powershell', 'bash', 'login-shell', 'wsl']);
export const profileIdSchema = z.string().min(1).max(200).refine(s => !/[\u0000-\u001f\u007f]/.test(s));
const utf8Size = (s: string) => new TextEncoder().encode(s).byteLength;
export const timestampSchema = z.iso.datetime({ offset: false });
const text = (max: number) => z.string().max(max).refine(s => !/[\u0000-\u001f\u007f]/.test(s), 'Control characters are not allowed');
export const titleSchema = text(200).min(1).refine(s => s.trim().length > 0);
export const isWslUncPath = (s: string) => /^\\\\(?:wsl\.localhost|wsl\$)\\[^\\/]+\\/i.test(s);
export const pathSchema = text(32767).min(1).refine(s => isWslUncPath(s) || (/^[A-Za-z]:[\\/]/.test(s) || /^\/(?!\/)/.test(s)) && !/^[/\\]{2}/.test(s) && !s.includes('://'), 'Expected an absolute local path');
export const statusSchema = z.enum(['waiting', 'error', 'running', 'unknown', 'settled']);
export const tabStatusSchema = z.enum(['waiting', 'error', 'running', 'unknown']);
export const lifecycleSchema = z.enum(['launching', 'open', 'closed', 'launch-uncertain']);
export const errorSchema = z.object({ code: z.enum(['VALIDATION', 'NOT_FOUND', 'UNSUPPORTED', 'DEPENDENCY_MISSING', 'LAUNCH_FAILED', 'REGISTRATION_TIMEOUT', 'TARGET_LOST', 'TARGET_AMBIGUOUS', 'FOCUS_DENIED', 'MONITOR_UNAVAILABLE', 'NATIVE_UNAVAILABLE', 'AUTH_FAILED', 'SETTLE_CONFIRM_REQUIRED', 'RETRY_CONFIRM_REQUIRED', 'STORAGE_FAILED', 'INTERNAL']), message: text(1000).min(1), retryable: z.boolean() }).strict();
export const resultSchema = <T extends z.ZodType>(value: T) => z.discriminatedUnion('ok', [z.object({ ok: z.literal(true), value }).strict(), z.object({ ok: z.literal(false), error: errorSchema }).strict()]);
const count = z.number().int().min(0).max(1000000);
export const capabilitiesSchema = z.object({ createWindow: z.boolean(), addTab: z.boolean(), focusWindow: z.boolean(), activateTab: z.boolean(), splitPane: z.literal(false), attachExisting: z.literal(false), closeTerminal: z.boolean(), commandExitStatus: z.literal(false), processTracking: z.boolean(), explorerContextMenu: z.boolean(), embeddedTerminal: z.boolean().optional(), terminalLifetime: z.enum(['app-owned', 'external-legacy']).optional(), shellSurvival: z.boolean().optional() }).strict();
const processRuleIdSchema = z.union([idSchema, z.string().max(120).regex(/^custom-[a-z0-9][a-z0-9-]*-[a-z0-9]+$/)]);
export const processRuleSchema = z.object({ id: processRuleIdSchema, label: titleSchema, enabled: z.boolean(), executableBasenames: z.array(text(260).min(1).refine(s => s.trim().length > 0 && !/[\\/:]/.test(s))).max(30), executablePaths: z.array(pathSchema.max(32760)).max(30), scriptPathSuffixes: z.array(text(4096).min(1).refine(s => s.trim().length > 0 && !s.split(/[\\/]/).some(component => component === '.' || component === '..'), 'Script suffixes must contain path components, not traversal')).max(30) }).strict().refine(r => r.executableBasenames.length + r.executablePaths.length > 0);
export const settingsSchema = z.object({ version: z.literal(1), pythonPath: pathSchema.nullable().default(null), accentColor: z.string().regex(/^#[0-9a-fA-F]{6}$/), adapterId: adapterSchema, shellId: shellIdSchema, shellExecutable: pathSchema.nullable(), terminalProfileId: profileIdSchema.nullable().optional(), processRules: z.array(processRuleSchema).max(100).refine(r => new Set(r.map(x => x.id)).size === r.length), historyPageSize: z.number().int().min(1).max(100) }).strict();
export const probeSchema = z.object({ platform: text(100), arch: text(100), adapterId: adapterSchema, available: z.boolean(), python: z.object({ detected: pathSchema.nullable(), usable: z.boolean(), reason: text(1000).nullable() }).strict().optional(), terminalVersion: text(100).nullable(), capabilities: capabilitiesSchema, shells: z.array(z.object({ id: shellIdSchema, executable: pathSchema, available: z.boolean(), reason: text(1000).nullable() }).strict()).max(100), reasons: z.array(text(1000)).max(100) }).strict();
export const explorerSchema = z.object({ supported: z.boolean(), installed: z.boolean(), folderItemInstalled: z.boolean(), backgroundInstalled: z.boolean(), reason: text(1000).nullable() }).strict();
// Keep previously legal stored data readable so the editor can report and repair it.
// New writes and PTY/WSL transport use the stricter, single-line schema below.
export const storedEnvVarSchema = z.object({ name: z.string().trim().min(1).max(256).refine(s => !/[=\u0000]/.test(s)), value: z.string().max(32767).refine(s => !s.includes('\u0000')) }).strict();
export const envVarSchema = z.object({ name: z.string().trim().min(1).max(256).refine(s => !/[=:\/\s\u0000]/u.test(s), 'Names cannot contain whitespace, =, :, / or NUL'), value: z.string().max(32767).refine(s => !/[\r\n\u0000]/.test(s), 'Values must be single-line with no CR, LF or NUL') }).strict();
export const envVarsSchemaFor = (windows: boolean) => z.array(envVarSchema).max(200).refine(vars => new Set(vars.map(v => windows ? v.name.toLowerCase() : v.name)).size === vars.length, 'Duplicate environment variable names');
export const storedEnvVarsSchema = z.array(storedEnvVarSchema).max(200).refine(vars => new Set(vars.map(v => typeof process === 'undefined' || process.platform === 'win32' ? v.name.toLowerCase() : v.name)).size === vars.length, 'Duplicate environment variable names');
export const envVarsSchema = envVarsSchemaFor(typeof process === 'undefined' || process.platform === 'win32');
export const cliIntegrationSchema = z.object({ supported: z.boolean(), installed: z.boolean(), command: z.literal('shellfox start <path>'), reason: text(1000).nullable() }).strict();
export const tabSchema = z.object({ id: idSchema, sessionId: idSchema, title: titleSchema, cwd: pathSchema, ordinal: count, createdAt: timestampSchema, lifecycle: lifecycleSchema, status: tabStatusSchema, agents: count, monitoringReason: text(1000).nullable(), error: errorSchema.nullable(), generation: idSchema.optional(), terminalKind: z.enum(['embedded', 'external-legacy']).optional(), profileId: profileIdSchema.nullable().optional(), exitCode: z.number().int().nullable().optional() }).strict();
export const sessionWindowSchema = z.object({ state: z.enum(['alive', 'closed', 'unknown', 'opening', 'launch-uncertain', 'unsupported']), canReopen: z.boolean(), canRegister: z.boolean(), reason: text(1000) }).strict();
export const registrationGuideSchema = z.object({ command: text(65536).min(1), expiresAt: timestampSchema, titleMarker: text(200), instructions: text(1000) }).strict();
export const sessionSchema = z.object({ id: idSchema, title: titleSchema, cwd: pathSchema, adapterId: adapterSchema, shellId: shellIdSchema, createdAt: timestampSchema, updatedAt: timestampSchema, settledAt: timestampSchema.nullable(), status: statusSchema, activityStatus: tabStatusSchema, counts: z.object({ running: count, waiting: count, unknown: count, error: count, closed: count, agents: count }).strict(), tabs: z.array(tabSchema).max(1000), env: storedEnvVarsSchema.default([]), window: sessionWindowSchema.optional(), canFocus: z.boolean(), canAddTab: z.boolean(), controlReason: text(1000).nullable(), error: errorSchema.nullable(), terminalLifetime: z.enum(['app-owned', 'external-legacy']).optional(), shellSurvival: z.boolean().optional() }).strict();
export const historyQuerySchema = z.object({ search: text(500), status: z.enum(['all', 'waiting', 'error', 'running', 'unknown', 'settled']), page: z.number().int().min(1).max(1000000), pageSize: z.number().int().min(1).max(100) }).strict();
export const historyPageSchema = z.object({ items: z.array(sessionSchema).max(100), total: count, page: z.number().int().min(1), pageSize: z.number().int().min(1).max(100) }).strict();
export const snapshotSchema = z.object({ revision: z.number().int().min(0).max(Number.MAX_SAFE_INTEGER), sessions: z.array(sessionSchema).max(10000), settings: settingsSchema, probe: probeSchema, explorer: explorerSchema, cli: cliIntegrationSchema }).strict();
export const changedEventSchema = z.object({ revision: z.number().int().min(0).max(Number.MAX_SAFE_INTEGER), reason: z.enum(['sessions', 'settings', 'native', 'history']), selectSessionId: idSchema.optional() }).strict();
export const processIdentitySchema = z.object({ pid: z.number().int().min(1).max(4294967295), startTime: z.string().regex(/^\d{1,80}$/) }).strict();
export const registrationSchema = z.object({ sessionId: idSchema, tabId: idSchema, operationId: idSchema, shell: processIdentitySchema, shellExecutable: pathSchema, cwd: pathSchema, registeredAt: timestampSchema }).strict();
export const windowTargetSchema = z.object({ kind: z.literal('windows-terminal'), windowName: text(100).min(1), hwnd: z.string().regex(/^\d{1,20}$/), owner: processIdentitySchema, sessionId: idSchema, markerPrefix: text(200).min(1), verification: z.literal('native-title') }).strict();
export const launchReceiptSchema = z.object({ operationId: idSchema, dispatch: z.literal('started'), target: windowTargetSchema.nullable() }).strict();
export const observationSchema = z.object({ sessionId: idSchema, tabId: idSchema, observedAt: timestampSchema, root: z.enum(['alive', 'exited', 'unavailable']), health: z.enum(['healthy', 'unknown']), agents: count, reason: text(1000).nullable() }).strict();
export const nativeEventSchema = z.discriminatedUnion('type', [z.object({ type: z.literal('registered'), registration: registrationSchema, target: windowTargetSchema.nullable() }).strict(), z.object({ type: z.literal('observations'), items: z.array(observationSchema).max(1000) }).strict(), z.object({ type: z.literal('target-lost'), sessionId: idSchema, reason: text(1000) }).strict(), z.object({ type: z.literal('operation-error'), sessionId: idSchema, tabId: idSchema, operationId: idSchema, error: errorSchema }).strict(), z.object({ type: z.literal('unavailable'), error: errorSchema }).strict()]);
export const terminalDataSchema = z.object({ type: z.literal('data'), tabId: idSchema, generation: idSchema, sequence: z.number().int().positive().max(Number.MAX_SAFE_INTEGER), data: z.string().min(1).max(16384).refine(s => utf8Size(s) <= 16384) }).strict();
export const terminalEventSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('activity'), tabId: idSchema, busy: z.boolean() }).strict(),
  terminalDataSchema,
  z.object({ type: z.literal('exit'), tabId: idSchema, generation: idSchema, exitCode: z.number().int().nullable(), signal: z.number().int().nullable(), lastSequence: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER) }).strict(),
  z.object({ type: z.literal('error'), tabId: idSchema, generation: idSchema, error: errorSchema }).strict(),
]);
export const terminalProfileSchema = z.object({ id: profileIdSchema, label: titleSchema, environment: z.enum(['local', 'wsl']), executable: pathSchema, args: z.array(z.string().max(4096)).max(30), distro: text(200).min(1).nullable(), available: z.boolean(), unavailableReason: text(1000).nullable().optional(), canTerminateDescendants: z.boolean().optional() }).strict();
export const terminalProfilesSchema = z.object({ profiles: z.array(terminalProfileSchema).max(100), defaultProfileId: profileIdSchema.nullable(), lifetime: z.literal('app-owned'), shellSurvival: z.literal(false) }).strict();
export const terminalAttachmentSchema = z.object({ tabId: idSchema, sessionId: idSchema, generation: idSchema, firstSequence: z.number().int().positive().max(Number.MAX_SAFE_INTEGER), lastSequence: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER), chunks: z.array(terminalDataSchema).max(8192), truncated: z.boolean(), state: z.enum(['open', 'closed']), exitCode: z.number().int().nullable(), cols: z.number().int().min(2).max(500), rows: z.number().int().min(2).max(500), lifetime: z.literal('app-owned') }).strict().refine(a => a.chunks.reduce((sum, c) => sum + utf8Size(c.data), 0) <= 262144 && a.firstSequence <= a.lastSequence + 1 && a.chunks.every((c,i) => c.tabId === a.tabId && c.generation === a.generation && c.sequence >= a.firstSequence && c.sequence <= a.lastSequence && (!i || c.sequence > a.chunks[i-1].sequence)));
const terminalIdentity = { tabId: idSchema, generation: idSchema };
const empty = z.object({}).strict();
const sessionId = z.object({ sessionId: idSchema }).strict();
export const requestSchemas = {
  getSnapshot: empty,
  copyText: z.object({ text: z.string().max(32768).refine(s => utf8Size(s) <= 32768) }).strict(),
  openSessionFolder: sessionId,
  setSessionEnv: z.object({ sessionId: idSchema, env: envVarsSchema }).strict(),
  setCliIntegration: z.object({ installed: z.boolean() }).strict(),
  createSession: z.object({ title: titleSchema.optional(), cwd: pathSchema, requestId: idSchema }).strict(),
  addTab: z.object({ sessionId: idSchema, title: titleSchema.optional(), profileId: profileIdSchema.optional(), cwd: pathSchema.optional() }).strict(),
  getTerminalProfiles: empty,
  attachTerminal: z.object({ tabId: idSchema, afterSequence: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER).optional(), generation: idSchema.optional() }).strict(),
  writeTerminal: z.object({ ...terminalIdentity, data: z.string().min(1).max(65536).refine(s => !s.includes('\u0000') && utf8Size(s) <= 65536) }).strict(),
  resizeTerminal: z.object({ ...terminalIdentity, cols: z.number().int().min(2).max(500), rows: z.number().int().min(2).max(500) }).strict(),
  closeTab: z.object(terminalIdentity).strict(),
  acknowledgeTerminal: z.object({ ...terminalIdentity, sequence: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER) }).strict(),
  detachTerminal: z.object(terminalIdentity).strict(),
  focusSession: sessionId,
  activateSession: sessionId, refreshSessionMembership: sessionId,
  prepareSessionRegistration: z.object({ sessionId: idSchema, shellId: shellIdSchema }).strict(),
  renameSession: z.object({ sessionId: idSchema, title: titleSchema }).strict(),
  settleSession: z.object({ sessionId: idSchema, confirmActive: z.boolean() }).strict(),
  unsettleSession: sessionId, clearSessionError: sessionId,
  retryTab: z.object({ tabId: idSchema, confirmPossibleDuplicate: z.boolean() }).strict(),
  getHistory: historyQuerySchema, saveSettings: settingsSchema,
  setExplorerIntegration: z.object({ installed: z.boolean() }).strict(), chooseDirectory: empty,
} as const;
export type ManagerMethod = keyof typeof requestSchemas;
export const requestEnvelopeSchema = z.object({ version: z.literal(1), method: z.enum(Object.keys(requestSchemas) as [ManagerMethod, ...ManagerMethod[]]), payload: z.unknown() }).strict();
export const responseSchemas = {
  copyText: resultSchema(z.object({ copied: z.literal(true) }).strict()),
  openSessionFolder: resultSchema(z.object({ opened: z.literal(true) }).strict()),
  setSessionEnv: resultSchema(sessionSchema),
  setCliIntegration: resultSchema(cliIntegrationSchema),
  getTerminalProfiles: resultSchema(terminalProfilesSchema), attachTerminal: resultSchema(terminalAttachmentSchema),
  acknowledgeTerminal: resultSchema(z.object({ acknowledged: z.literal(true) }).strict()), detachTerminal: resultSchema(z.object({ detached: z.literal(true) }).strict()),
  writeTerminal: resultSchema(z.object({ written: z.literal(true) }).strict()), resizeTerminal: resultSchema(z.object({ resized: z.literal(true) }).strict()), closeTab: resultSchema(sessionSchema),
  activateSession: resultSchema(sessionSchema), refreshSessionMembership: resultSchema(sessionSchema), prepareSessionRegistration: resultSchema(registrationGuideSchema),
  getSnapshot: resultSchema(snapshotSchema), createSession: resultSchema(sessionSchema), addTab: resultSchema(sessionSchema), focusSession: resultSchema(z.object({ focused: z.literal(true) }).strict()), renameSession: resultSchema(sessionSchema), settleSession: resultSchema(sessionSchema), unsettleSession: resultSchema(sessionSchema), clearSessionError: resultSchema(sessionSchema), retryTab: resultSchema(sessionSchema), getHistory: resultSchema(historyPageSchema), saveSettings: resultSchema(settingsSchema), setExplorerIntegration: resultSchema(explorerSchema), chooseDirectory: resultSchema(z.object({ cwd: pathSchema }).strict().nullable()),
} as const;
// Stable descriptive aliases for callers consuming native validation.
export const shellRegistrationSchema = registrationSchema;
export const nativeProbeSchema = probeSchema;
export const explorerIntegrationSchema = explorerSchema;
export const tabObservationSchema = observationSchema;
