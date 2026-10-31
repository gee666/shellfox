let pending = 0;

export function isAttachmentPickerOpen(): boolean { return pending > 0; }

/** The native picker can background Android or make iOS inactive without leaving the terminal. */
export async function withAttachmentPicker<T>(pick: () => Promise<T>): Promise<T> {
  ++pending;
  try { return await pick(); }
  finally { --pending; }
}
