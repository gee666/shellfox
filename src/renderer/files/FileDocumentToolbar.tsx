import type { ReactNode } from 'react';
import type { ManagerApi } from '../../shared/contracts';
import type { FileLocation } from '../../shared/file-contracts';
import { CopyFilePathButton } from './CopyFilePathButton';
import { OpenFileExplorerButton } from './OpenFileExplorerButton';
import { FileTransfers } from './FileTransfers';
import { fileName } from './file-preview-utils';

function ControlIcon({ path }: { path: string }) {
  return <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" aria-hidden="true"><path d={path} /></svg>;
}

export function FileDocumentToolbar({ api, location, folderPath, dirty, saving, onSave, onReload, onUploaded,
  native, remote, wrap, onToggleWrap, copyPath, children }: {
  api: ManagerApi; location: FileLocation; folderPath: string; dirty: boolean; saving: boolean;
  onSave?: () => void; onReload: () => void; onUploaded: () => void; native?: boolean; remote: boolean;
  wrap?: boolean; onToggleWrap: () => void; copyPath: boolean; children?: ReactNode;
}) {
  return <div className="file-document-toolbar" role="toolbar" aria-label="File controls">
    <span className="file-document-path" title={location.path}>{fileName(location.path)}{dirty ? ' *' : ''}</span>
    {copyPath && <CopyFilePathButton api={api} path={location.path} />}
    {onSave && <button type="button" className="icon-button" aria-label="Save file" title={saving ? 'Saving file…' : 'Save file'} disabled={!dirty || saving} onClick={onSave}>
      <ControlIcon path="M2 2h10l2 2v10H2V2ZM5 2v4h6V2M5 14V9h6v5" />
    </button>}
    <button type="button" className="icon-button" aria-label="Reload file" title="Reload file" disabled={saving} onClick={onReload}>
      <ControlIcon path="M13 6A5 5 0 1 0 13 10M13 2v4H9" />
    </button>
    {remote && <FileTransfers inline api={api} location={{ ...location, path: folderPath }} selectedPath={location.path} onUploaded={onUploaded} />}
    {native && <OpenFileExplorerButton api={api} location={location} label="Show file in system explorer" />}
    {wrap !== undefined && <button type="button" className="icon-button" aria-label="Wrap lines" aria-pressed={wrap}
      title={wrap ? 'Disable line wrapping' : 'Wrap long lines'} onClick={onToggleWrap}>
      <ControlIcon path="M2 3h12M2 7h9a3 3 0 0 1 0 6H8m2-2-2 2 2 2M2 11h3" />
    </button>}
    {children}
  </div>;
}
