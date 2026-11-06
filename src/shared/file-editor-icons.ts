export const fileEditorIconPaths = {
  close: 'M6 6l12 12M18 6 6 18',
  more: 'M12 4a1 1 0 1 0 0 2 1 1 0 0 0 0-2M12 11a1 1 0 1 0 0 2 1 1 0 0 0 0-2M12 18a1 1 0 1 0 0 2 1 1 0 0 0 0-2',
  save: 'M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h12l4 4v12a2 2 0 0 1-2 2ZM7 3v6h10V3M7 21v-8h10v8',
  'rotate-left': 'M3 10a9 9 0 1 1 2.6 8.4M3 4v6h6M10 9h5v6h-5z',
  'rotate-right': 'M21 10a9 9 0 1 0-2.6 8.4M21 4v6h-6M9 9h5v6H9z',
  'flip-horizontal': 'M12 3v2m0 3v2m0 4v2m0 3v2M3 6h5v12H3zM21 6h-5v12h5z',
  'flip-vertical': 'M3 12h2m3 0h2m4 0h2m3 0h2M6 3v5h12V3zM6 21v-5h12v5z',
  reset: 'M3 5v6h6M3.5 11a9 9 0 1 1 2.3 7',
  crop: 'M6 3v14a1 1 0 0 0 1 1h14M3 6h14a1 1 0 0 1 1 1v14',
} as const;
export type FileEditorIconName = keyof typeof fileEditorIconPaths;
/** Application-owned path data only. Never accepts file markup. */
export function fileEditorIconSvg(name: FileEditorIconName): string {
  return `<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="${fileEditorIconPaths[name]}"></path></svg>`;
}
