// Explicit QA bundle only. Original clipboard text stays in this process's memory.
import { useEffect, useRef } from 'react';
import Clipboard from '@react-native-clipboard/clipboard';

export function useQaClipboard() {
  const state = useRef<{ original?: string; owned?: string; ready: Promise<void>; restorable: boolean }>({ ready: Promise.resolve(), restorable: false });
  const setter = useRef(Clipboard.setString);
  const restore = async () => {
    const current = state.current; await current.ready;
    if (current.original !== undefined && current.owned !== undefined && await Clipboard.getString() === current.owned) setter.current(current.original);
    current.owned = undefined;
  };
  useEffect(() => {
    const current = state.current, originalSet = Clipboard.setString;
    setter.current = originalSet;
    current.ready = (async () => {
      // Android cannot restore non-text clipboard formats through this library. Do not replace them.
      if (await Clipboard.hasString()) { current.original = await Clipboard.getString(); current.restorable = true; }
    })().catch(() => {}).then(() => { console.info(`FILES_QA_CLIPBOARD_READY ${current.restorable}`); });
    const trackedSet = (value: string) => {
      if (!current.restorable) throw new Error('QA clipboard copy requires a restorable text snapshot.');
      originalSet(value); current.owned = value;
    };
    Clipboard.setString = trackedSet;
    return () => {
      if (Clipboard.setString === trackedSet) Clipboard.setString = originalSet;
      void restore().catch(() => {});
    };
  }, []);
  const readFixture = async () => {
    await state.current.ready;
    const owned = state.current.owned;
    return owned?.startsWith('/fixture/') && await Clipboard.getString() === owned ? `Clipboard: ${owned}` : 'Clipboard fixture has not been copied.';
  };
  return { restore, readFixture };
}
