import { useEffect, useRef, useState } from 'react';
import { v4 as uuid } from 'uuid';
import type { FileApi, FileLocation } from '../../src/shared/file-contracts';
import { pickedFiles } from './files';
import { downloads } from './downloads';
import { FileTransferCancelled, MobileFileTransfer, streamingFiles, type FileTransferProgress } from './file-transfer';

export function useFileTransfers({ api, context, available, connectionEpoch, uploaded }: {
  api: FileApi;
  context: Omit<FileLocation, 'path'>;
  available: boolean;
  connectionEpoch?: string | null;
  uploaded: () => void;
}) {
  const active = useRef<MobileFileTransfer | null>(null);
  const mounted = useRef(true);
  const connected = useRef(available); connected.current = available;
  const [progress, setProgress] = useState<FileTransferProgress | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    mounted.current = true; setProgress(null);
    return () => { mounted.current = false; active.current?.cancel(); active.current = null; };
  }, [api, connectionEpoch]);
  useEffect(() => { if (!available) active.current?.cancel(); }, [available]);
  const run = async (kind: 'upload' | 'download', path: string) => {
    if (active.current || !available) return;
    const wire = streamingFiles(api);
    if (!wire) { setError('Upgrade the host to transfer files.'); return; }
    setError(null); setMessage(null);
    const transfer: MobileFileTransfer = new MobileFileTransfer({ api: wire, context: { ...context }, files: pickedFiles, downloads, token: uuid(),
      current: () => mounted.current && connected.current && active.current === transfer,
      progress: value => { if (mounted.current && active.current === transfer) setProgress(value); },
    });
    active.current = transfer;
    try {
      if (kind === 'upload') {
        const result = await transfer.upload(path);
        if (result && mounted.current && active.current === transfer) { setMessage(`Uploaded ${result}`); uploaded(); }
      } else {
        const result = await transfer.download(path);
        if (result && mounted.current && active.current === transfer) setMessage(`Saved to ${result.location}: ${result.fileName}`);
      }
    } catch (cause) {
      if (mounted.current && active.current === transfer) {
        if (cause instanceof FileTransferCancelled) setMessage('Transfer cancelled.');
        else setError(cause instanceof Error ? cause.message : 'File transfer failed.');
      }
    } finally { if (active.current === transfer) { active.current = null; if (mounted.current) setProgress(null); } }
  };
  return { progress, running: progress !== null, message, error, supported: streamingFiles(api) !== null,
    cancel: () => active.current?.cancel(),
    upload: (path: string) => { void run('upload', path); }, download: (path: string) => { void run('download', path); },
    dismiss: () => { setMessage(null); setError(null); },
  };
}
