import path from 'node:path';
import { lstat, readFile } from 'node:fs/promises';
import { z } from 'zod';
import { CliFailure } from './output';
const relativePath = z.string().min(1).max(4096).refine(value => !/[\0\r\n]/.test(value) && !path.posix.isAbsolute(value) && !path.win32.isAbsolute(value));
const manifestSchema = z.object({ version: z.literal(1), node: relativePath, entry: relativePath, resources: relativePath, runAsNode: z.boolean(), headless: z.boolean() }).strict();
export interface RuntimeContext { resourcesPath?: string; projectRoot?: string; runtimePath?: string; headless: boolean }
/** Only a manifest beside this entry's known package layout may override inherited context. */
export async function resolveRuntimeContext(entry: string, env: NodeJS.ProcessEnv = process.env): Promise<RuntimeContext> {
  const root = path.dirname(path.dirname(entry)), manifest = path.join(root, 'runtime.json');
  const fallback = { resourcesPath: env.SHELLFOX_RESOURCES_PATH, projectRoot: env.SHELLFOX_PROJECT_ROOT, runtimePath: env.SHELLFOX_RUNTIME_PATH, headless: env.SHELLFOX_HEADLESS === '1' };
  let info;
  try { info = await lstat(manifest); }
  catch (error) { if (error instanceof Error && 'code' in error && error.code === 'ENOENT') return fallback; throw error; }
  if (!info.isFile() || info.isSymbolicLink() || info.size > 4096) throw new CliFailure('VALIDATION', 'Unsafe packaged CLI runtime manifest.');
  let raw: unknown;
  try { raw = JSON.parse(await readFile(manifest, 'utf8')); }
  catch { throw new CliFailure('VALIDATION', 'Invalid packaged CLI runtime manifest.'); }
  const parsed = manifestSchema.safeParse(raw);
  if (!parsed.success) throw new CliFailure('VALIDATION', 'Invalid packaged CLI runtime manifest.');
  const value = parsed.data;
  const expectedEntry = value.headless ? 'app/main.cjs' : 'main/cli.cjs';
  const expectedResources = value.headless ? 'resources' : '..';
  const normalize = (file: string) => process.platform === 'win32' ? path.resolve(file).toLowerCase() : path.resolve(file);
  if (value.entry !== expectedEntry || value.resources !== expectedResources || normalize(path.join(root, expectedEntry)) !== normalize(entry)) throw new CliFailure('VALIDATION', 'Runtime manifest does not match this CLI entry.');
  return { resourcesPath: path.resolve(root, value.resources), runtimePath: value.headless ? root : undefined, headless: value.headless };
}
export function applyRuntimeContext(context: RuntimeContext): () => void {
  const updates = { SHELLFOX_RESOURCES_PATH: context.resourcesPath, SHELLFOX_PROJECT_ROOT: context.projectRoot, SHELLFOX_RUNTIME_PATH: context.runtimePath,
    SHELLFOX_HEADLESS: context.headless ? '1' : undefined };
  const previous = new Map<string, string | undefined>();
  for (const [key, value] of Object.entries(updates)) {
    previous.set(key, process.env[key]);
    if (value === undefined) delete process.env[key]; else process.env[key] = value;
  }
  return () => { for (const [key, value] of previous) { if (value === undefined) delete process.env[key]; else process.env[key] = value; } };
}
