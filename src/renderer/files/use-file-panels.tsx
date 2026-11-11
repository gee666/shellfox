import { useRef, useState } from 'react';
import type { ManagerApi } from '../../shared/contracts';
import type { FileList } from '../../shared/file-contracts';
import type { FileDirectoryRequest } from './file-directory-request';
import { FileSidebar, type FileOpenRequest } from './FileSidebar';

interface HostTarget { hostId: string; api: ManagerApi; directory?: string; allowNativeBrowse?: boolean; scope?: { sessionId: string; tabId?: string } }
interface FilePanel extends HostTarget { open: boolean; browserOpen?: boolean; request?: FileOpenRequest; directoryRequest?: FileDirectoryRequest }

/** Host panels outlive terminal sessions, including sessions that have been deleted. */
export function useFilePanels() {
  const [panels, setPanels] = useState<Record<string, FilePanel>>({});
  const currentPanels = useRef(panels); currentPanels.current = panels;
  const toggleBrowser = (hostId: string): boolean => {
    if (!currentPanels.current[hostId]?.open) return false;
    setPanels(previous => {
      const panel = previous[hostId];
      return panel?.open ? { ...previous, [hostId]: { ...panel, browserOpen: !(panel.browserOpen ?? true) } } : previous;
    });
    return true;
  };
  const toggle = (target: HostTarget) => {
    if (!target.api.files) return;
    setPanels(previous => {
      const current = previous[target.hostId];
      // Remote wrappers delegate by host identity, so replacing one on reconnect must not orphan its drafts.
      const panel = current ? { ...current, open: !current.open } : { ...target, open: true };
      return { ...previous, [target.hostId]: panel };
    });
  };
  const openFile = (target: HostTarget, request: FileOpenRequest) => {
    if (!target.api.files) return;
    setPanels(previous => {
      const current = previous[target.hostId];
      const panel = current ? { ...current, ...target, api: current.api, open: true, request, directoryRequest: undefined } : { ...target, open: true, request };
      return { ...previous, [target.hostId]: panel };
    });
  };
  const openDirectory = (target: HostTarget, listing: FileList) => {
    if (!target.api.files) return;
    const directoryRequest: FileDirectoryRequest = { path: listing.path, id: crypto.randomUUID(), listing };
    setPanels(previous => {
      const current = previous[target.hostId];
      const sameScope = current?.scope?.sessionId === target.scope?.sessionId && current?.scope?.tabId === target.scope?.tabId;
      const panel: FilePanel = { ...current, ...target, api: current?.api ?? target.api, directory: listing.path,
        open: true, directoryRequest, request: sameScope ? current?.request : undefined };
      return { ...previous, [target.hostId]: panel };
    });
  };
  const close = (hostId: string) => setPanels(previous => {
    const current = previous[hostId];
    return current ? { ...previous, [hostId]: { ...current, open: false } } : previous;
  });
  const render = (activeHostId: string) => Object.values(panels).map(panel => <FileSidebar key={panel.hostId}
    api={panel.api} sessionId={panel.scope?.sessionId} tabId={panel.scope?.tabId} initialDirectory={panel.directory}
    allowNativeBrowse={panel.allowNativeBrowse} openRequest={panel.request} directoryRequest={panel.directoryRequest}
    browserOpen={panel.browserOpen ?? true} onBrowserChange={browserOpen => setPanels(previous => {
      const current = previous[panel.hostId];
      return current ? { ...previous, [panel.hostId]: { ...current, browserOpen } } : previous;
    })}
    onHostDirectory={path => setPanels(previous => {
      const current = previous[panel.hostId];
      return current ? { ...previous, [panel.hostId]: { ...current, scope: undefined, directory: path, request: undefined, directoryRequest: undefined } } : previous;
    })}
    hidden={panel.hostId !== activeHostId || !panel.open} onClose={() => close(panel.hostId)} />);
  return { toggle, toggleBrowser, openFile, openDirectory, render, isOpen: (hostId: string) => !!panels[hostId]?.open };
}
