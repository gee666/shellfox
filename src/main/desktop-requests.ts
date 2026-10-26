import { clipboard, dialog, shell, type BrowserWindow } from 'electron';
import { failure, success } from '../shared/contracts';
import { CLIPBOARD_PASTE_BYTES, requestSchemas, type ManagerMethod } from '../shared/schemas';
import { validateDirectory } from './directory';
import type { UpdateSource } from './update/self-updater';

interface DesktopRequests {
  window: BrowserWindow;
  updates?: UpdateSource;
  sessionDirectory(sessionId: string): string | null | Promise<string | null>;
}
/** Desktop-only operations stay in Electron, even when another process owns the host. */
export async function desktopRequest(method: ManagerMethod, payload: unknown, options: DesktopRequests): Promise<unknown> {
  switch (method) {
    case 'copyText':
      try { await clipboard.writeText(requestSchemas.copyText.parse(payload).text); return success({ copied: true }); }
      catch { return failure('INTERNAL', 'Could not copy text to the system clipboard.', true); }
    case 'readClipboardText':
      try {
        const text = await clipboard.readText();
        return new TextEncoder().encode(text).byteLength > CLIPBOARD_PASTE_BYTES
          ? failure('VALIDATION', 'Clipboard text is too large to paste (limit 64 KiB). Paste smaller sections.')
          : success({ text });
      } catch { return failure('INTERNAL', 'Could not read the system clipboard.', true); }
    case 'openExternal':
      try { await shell.openExternal(new URL(requestSchemas.openExternal.parse(payload).url).href); return success({ opened: true }); }
      catch { return failure('INTERNAL', 'The link could not be opened.'); }
    case 'openSessionFolder': {
      const cwd = await options.sessionDirectory(requestSchemas.openSessionFolder.parse(payload).sessionId);
      if (!cwd) return failure('NOT_FOUND', 'Session not found.');
      try {
        const error = await shell.openPath(await validateDirectory(cwd));
        return error ? failure('INTERNAL', 'The session folder could not be opened: ' + error) : success({ opened: true });
      } catch { return failure('NOT_FOUND', 'The session folder is missing or inaccessible.'); }
    }
    case 'chooseDirectory': {
      const selection = await dialog.showOpenDialog(options.window, { properties: ['openDirectory'], title: 'Choose default working directory' });
      if (selection.canceled || !selection.filePaths[0]) return success(null);
      try { return success({ cwd: await validateDirectory(selection.filePaths[0]) }); }
      catch { return failure('VALIDATION', 'Choose an accessible local directory'); }
    }
    case 'chooseSshKeyFile': {
      const selection = await dialog.showOpenDialog(options.window, { properties: ['openFile'], title: 'Choose SSH private key' });
      return success(selection.canceled || !selection.filePaths[0] ? null : { path: selection.filePaths[0] });
    }
    case 'getUpdateStatus': return options.updates ? success(await options.updates.status()) : failure('UNSUPPORTED', 'Update checks are unavailable.');
    case 'downloadUpdate': return options.updates?.download ? options.updates.download() : failure('UNSUPPORTED', 'In-app updates are unavailable.');
    case 'installUpdate': return options.updates?.install ? options.updates.install() : failure('UNSUPPORTED', 'In-app updates are unavailable.');
    default: return undefined;
  }
}
