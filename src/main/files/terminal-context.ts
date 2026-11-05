import type { RepositoryPort } from '../models';
import type { PtyBackend } from '../terminal/backend';
import { FileError, type FileSessionContext } from './paths';

export function terminalFileContext(repository: RepositoryPort, backend: PtyBackend, refresh?: () => Promise<void> | undefined): FileSessionContext {
  const terminal = (id: string) => {
    const owned = backend.get(id); return owned?.generation === repository.tab(id)?.operationId ? owned : undefined;
  };
  return {
    session: id => repository.session(id), tab: id => repository.tab(id), terminal,
    resolveDirectory: async id => {
      const before = terminal(id);
      if (!before) throw new FileError('UNSUPPORTED', 'This tab no longer has a running shell. Browse Home or enter an absolute folder.');
      let result = await backend.currentDirectory(id);
      if (!result.ok && result.error.code === 'MONITOR_UNAVAILABLE' && refresh) { await refresh(); result = await backend.currentDirectory(id); }
      if (!result.ok) throw new FileError('UNSUPPORTED', result.error.message);
      if (terminal(id)?.generation !== before.generation) throw new FileError('UNSUPPORTED', 'Terminal changed while resolving its current directory.');
      return result.value.cwd;
    },
  };
}
