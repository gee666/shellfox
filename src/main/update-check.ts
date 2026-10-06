import type { UpdateStatusDto } from '../shared/contracts';

export const UPDATE_COMMAND = 'shellfox update';
export const RELEASES_URL = 'https://github.com/gee666/shellfox/releases';
export const LATEST_RELEASE_URL = 'https://api.github.com/repos/gee666/shellfox/releases/latest';
export interface FetchResponse { ok: boolean; status: number; json(): Promise<unknown> }
export type FetchLike = (url: string, init: { headers: Record<string, string>; signal: AbortSignal }) => Promise<FetchResponse>;

const VERSION_PATTERN = /^\d+(\.\d+)*([-+][0-9A-Za-z.+-]*)?$/;
const core = (version: string) => version.trim().replace(/^v/, '').split(/[-+]/, 1)[0];
const part = (parts: string[], index: number) => { const match = /^\d+/.exec(parts[index] ?? ''); return match ? BigInt(match[0]) : 0n; };

/** Dotted numeric compare (same semantics as shellfox-update.sh): leading "v" and -/+ suffixes are ignored. */
export function isNewer(candidate: string, current: string): boolean {
  const a = core(candidate).split('.'), b = core(current).split('.');
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = part(a, i), y = part(b, i);
    if (x !== y) return x > y;
  }
  return false;
}
/** Release tag (leading "v" allowed) to a plain version, or null when it is not a version. */
export function parseVersion(tag: unknown): string | null {
  if (typeof tag !== 'string') return null;
  const version = tag.trim().replace(/^v/, '');
  return version.length <= 64 && VERSION_PATTERN.test(version) ? version : null;
}
/** Latest published release version, or null on 404, network errors, timeouts and malformed answers. */
export async function fetchLatestRelease(fetchImpl: FetchLike, url = LATEST_RELEASE_URL, timeoutMs = 10_000): Promise<string | null> {
  const controller = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<null>(resolve => { timer = setTimeout(() => { controller.abort(); resolve(null); }, timeoutMs); });
  const lookup = (async () => {
    const response = await fetchImpl(url, { headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'shellfox-update-check' }, signal: controller.signal });
    if (!response.ok) return null;
    const body = await response.json();
    return parseVersion(body && typeof body === 'object' ? (body as { tag_name?: unknown }).tag_name : null);
  })().catch(() => null);
  try { return await Promise.race([lookup, timeout]); } finally { if (timer) clearTimeout(timer); }
}

/** Performs at most one release lookup per application start; failures mean "no update known". */
export class UpdateChecker {
  private pending: Promise<UpdateStatusDto> | null = null;
  constructor(private readonly options: { current: string; check: () => Promise<string | null> }) {}
  status(): Promise<UpdateStatusDto> {
    this.pending ??= (async () => {
      const current = this.options.current;
      let latest: string | null = null;
      try { latest = parseVersion(await this.options.check()); } catch { /* Offline or blocked: stay silent. */ }
      return { current, latest, available: latest !== null && isNewer(latest, current), command: UPDATE_COMMAND, url: RELEASES_URL };
    })();
    return this.pending;
  }
}
