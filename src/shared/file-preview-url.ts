/** A separate document scheme avoids inheriting the app's script-blocking CSP. */
export const FILE_PREVIEW_SCHEME = 'shellfox-preview';
export const FILE_PREVIEW_CSP = "default-src 'none'; script-src 'unsafe-inline' data: blob:; style-src 'unsafe-inline' data: blob:; img-src data: blob:; font-src data: blob:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; sandbox allow-scripts";
export const FILE_PREVIEW_URL_LIMIT = 12 * 1024 * 1024;
export function desktopPreviewUrl(document: string): string {
  const url = `${FILE_PREVIEW_SCHEME}://document/?html=${encodeURIComponent(document)}`;
  if (url.length > FILE_PREVIEW_URL_LIMIT) throw new Error('Preview assets exceed the document size limit.');
  return url;
}
