import type { resolvePython } from '../../src/main/terminal/python';
import { denyAppProcess } from './app-process-model';

const profiles = new Set<string | null>();
export function registerFixturePython(configured: string | null = null): void {
  profiles.add(configured);
}
export function finishFixturePython(): void {
  profiles.clear();
}

export function fixturePythonResolver(resolve: typeof resolvePython): typeof resolvePython {
  return (configured = null, options, explicit = false) => {
    if (options?.exists || options?.run) {
      return resolve(configured, {
        ...options,
        exists: options.exists ?? (() => denyAppProcess('unmodeled Python executable check')),
        run: options.run ?? (() => denyAppProcess('unmodeled Python preflight')),
      }, explicit);
    }
    if (explicit || !profiles.has(configured)) return denyAppProcess('unregistered fixture Python preflight');
    // Fake PTY profiles do not independently discover a host Python. Keep real parser tests on injected ports.
    return resolve(configured, { env: {}, exists: async () => false, run: () => denyAppProcess('unmodeled Python preflight') });
  };
}
