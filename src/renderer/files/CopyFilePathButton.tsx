import type { ManagerApi } from '../../shared/contracts';
import { Icon, useAction, useNotify } from '../components';

export function CopyFilePathButton({ api, path }: { api: Partial<Pick<ManagerApi, 'copyText'>>; path: string }) {
  const action = useAction(), notify = useNotify();
  async function copy() {
    const copyText = api.copyText;
    if (!copyText) return;
    const result = await action.run(() => copyText({ text: path }));
    if (result?.ok) notify({ tone: 'info', message: 'Path copied' });
  }
  return <button type="button" className="icon-button" aria-label="Copy file path" title="Copy file path"
    disabled={!api.copyText || action.pending} onClick={() => void copy()}><Icon name="copy" /></button>;
}
