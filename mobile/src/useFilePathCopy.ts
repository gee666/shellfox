import { useEffect, useState, type MutableRefObject } from 'react';
import Clipboard from '@react-native-clipboard/clipboard';
import type { FileLocation } from '../../src/shared/file-contracts';
import type { BrowserFile, FileApi } from './file-browser-types';

export function useFilePathCopy({ api, context, file, target, operation, mounted, report }: {
  api: FileApi; context: Omit<FileLocation, 'path'>; file: BrowserFile | null; target: string | null;
  operation: MutableRefObject<number>; mounted: MutableRefObject<boolean>; report: (message: string) => void;
}) {
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1800);
    return () => clearTimeout(timer);
  }, [copied]);
  const copy = async () => {
    if (copying || !target) return;
    const version = operation.current;
    setCopying(true); setCopied(false);
    try {
      let canonical = file?.path;
      if (!canonical && api.beginRead && api.cancelTransfer) {
        const result = await api.beginRead({ ...context, path: target });
        if (!result.ok) throw new Error(result.error.message);
        try { canonical = result.value.path; }
        finally { await api.cancelTransfer({ transferId: result.value.transferId }).catch(() => {}); }
      }
      if (!mounted.current || version !== operation.current) return;
      if (!canonical) throw new Error('Open the file before copying its path.');
      await Clipboard.setString(canonical);
      if (mounted.current && version === operation.current) setCopied(true);
    } catch (cause) {
      if (mounted.current && version === operation.current) report(cause instanceof Error ? cause.message : 'Could not copy the file path.');
    } finally { if (mounted.current) setCopying(false); }
  };
  return { copied, copying, copy, clear: () => setCopied(false) };
}
