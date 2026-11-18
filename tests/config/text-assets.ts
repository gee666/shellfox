import { readFileSync } from 'node:fs';
/** Vite's trusted text transform; not an application process boundary. */
export const textAssets = { name: 'text-assets', enforce: 'pre' as const, transform: (_code: string, id: string) => /\.(sh|ps1)(\?.*)?$/.test(id) ? { code: 'export default ' + JSON.stringify(readFileSync(id.replace(/\?.*$/, ''), 'utf8')) + ';', map: null } : null };
