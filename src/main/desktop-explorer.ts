import { realpath, stat } from 'node:fs/promises';
import { shell } from 'electron';
import { failure, success, type ManagerSnapshot, type Result } from '../shared/contracts';
import type { FileLocation } from '../shared/file-contracts';
import { FileError, resolveFileTarget } from './files/paths';

/** Reveal files, but enter directories. Never launch a file's associated program. */
export async function openFileExplorer(input: FileLocation, snapshot: Pick<ManagerSnapshot, 'sessions'>): Promise<Result<null>> {
  try {
    const sessions = snapshot.sessions;
    const target = await resolveFileTarget({
      session: id => sessions.find(session => session.id === id),
      tab: id => {
        const tab = sessions.flatMap(session => session.tabs).find(tab => tab.id === id);
        return tab ? { ...tab, terminal: tab.profileId ? { profileId: tab.profileId } : null } : undefined;
      },
    }, input);
    const canonical = await realpath(target.native), info = await stat(canonical);
    if (info.isDirectory()) {
      const error = await shell.openPath(canonical);
      if (error) return failure('INTERNAL', 'The folder could not be opened: ' + error);
    } else if (info.isFile()) shell.showItemInFolder(canonical);
    else return failure('UNSUPPORTED', 'Only files and folders can be opened in the system file manager.');
    return success(null);
  } catch (error) {
    if (error instanceof FileError) return failure(error.code, error.message);
    return failure('NOT_FOUND', 'The file or folder is missing or inaccessible.');
  }
}
