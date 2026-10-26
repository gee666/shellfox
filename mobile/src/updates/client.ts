import { LATEST_RELEASE_URL, parseAndroidRelease, type AndroidUpdate } from './releases';

export interface UpdatesBridge {
  getVersion(): Promise<string>;
  /** Resolves when the system installer opens, not when installation finishes. */
  install(url: string, sha256: string): Promise<void>;
}
export interface UpdateClient {
  check(): Promise<AndroidUpdate | null>;
  install(): Promise<boolean>;
}

export function createUpdateClient(options: {
  platform: string; bridge: UpdatesBridge; fetch: typeof fetch; timeoutMs?: number;
}): UpdateClient {
  let installing = false;
  async function check(): Promise<AndroidUpdate | null> {
    if (options.platform !== 'android') return null;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), options.timeoutMs ?? 10_000);
    try {
      const current = await options.bridge.getVersion();
      const response = await options.fetch(LATEST_RELEASE_URL, {
        signal: controller.signal, redirect: 'error',
        headers: { Accept: 'application/vnd.github+json', 'Cache-Control': 'no-cache' },
      });
      if (!response.ok) throw new Error('Could not check for updates.');
      const length = response.headers.get('content-length');
      if (length !== null && (!/^\d+$/.test(length) || Number(length) > 2 * 1024 * 1024)) {
        throw new Error('Release metadata is too large.');
      }
      const text = await response.text();
      if (text.length > 2 * 1024 * 1024) throw new Error('Release metadata is too large.');
      const body: unknown = JSON.parse(text);
      return parseAndroidRelease(body, current);
    } finally { clearTimeout(timer); }
  }
  return {
    check,
    async install() {
      if (options.platform !== 'android') throw new Error('APK updates are only supported on Android.');
      if (installing) throw new Error('An update is already in progress.');
      installing = true;
      try {
        // Never authorize installation using a stale notice or a caller-supplied asset.
        const update = await check();
        if (!update) return false;
        await options.bridge.install(update.url, update.sha256);
        return true;
      } finally { installing = false; }
    },
  };
}
