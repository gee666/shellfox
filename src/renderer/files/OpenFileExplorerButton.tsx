import type { ManagerApi } from '../../shared/contracts';
import type { FileLocation } from '../../shared/file-contracts';
import { Icon, useAction } from '../components';

export function OpenFileExplorerButton({ api, location, label = 'Open in system explorer' }: {
  api: ManagerApi; location: FileLocation; label?: string;
}) {
  const action = useAction();
  const openFileExplorer = api.openFileExplorer;
  if (!openFileExplorer) return null;
  return <button type="button" className="icon-button" aria-label={label} title={label} disabled={action.pending}
    onClick={() => void action.run(() => openFileExplorer(location))}><Icon name="folder" /></button>;
}
