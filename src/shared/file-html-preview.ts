import { parse, serialize, type DefaultTreeAdapterMap } from 'parse5';

type Node = DefaultTreeAdapterMap['node'];
type Element = DefaultTreeAdapterMap['element'];
export type PreviewAssetLoader = (assetPath: string) => Promise<string | null>;
const MAX_ASSETS = 100;

function localPath(value: string, base = ''): string | null {
  const clean = value.trim();
  if (!clean || clean.startsWith('#') || /^(?:[a-z][a-z\d+.-]*:|\/\/|\/|\\)/i.test(clean)) return null;
  const withoutQuery = clean.split(/[?#]/, 1)[0]!;
  try { return base + decodeURIComponent(withoutQuery); } catch { return null; }
}
function decodeText(dataUrl: string): string | null {
  const match = /^data:[^,]*;base64,([a-z\d+/=]*)$/i.exec(dataUrl);
  if (!match) return null;
  try { return decodeURIComponent(Array.from(atob(match[1]!), char => `%${char.charCodeAt(0).toString(16).padStart(2, '0')}`).join('')); }
  catch { return null; }
}
function textDataUrl(text: string, mime = 'text/css'): string {
  return `data:${mime};charset=utf-8,${encodeURIComponent(text)}`;
}

/** Embed bounded project assets before the document enters an opaque, script-only sandbox.
 * The caller must apply its CSP and must not grant same-origin or a native bridge.
 */
export async function prepareFileHtml(html: string, load: PreviewAssetLoader): Promise<string> {
  const document = parse(html);
  const pending = new Map<string, Promise<string | null>>();
  let assetCharacters = 0;
  const asset = (path: string): Promise<string | null> => {
    const existing = pending.get(path);
    if (existing) return existing;
    if (pending.size >= MAX_ASSETS) return Promise.resolve(null);
    const result = load(path).catch(() => null).then(data => {
      assetCharacters += data?.length ?? 0;
      if (assetCharacters > 8 * 1024 * 1024) throw new Error('Preview assets exceed the 8 MiB document limit.');
      return data;
    });
    pending.set(path, result);
    return result;
  };
  async function css(source: string, base: string, depth = 0): Promise<string> {
    if (depth > 5) return '';
    // Resolve imports before url(), so imported CSS gets its own relative directory.
    let result = '';
    let cursor = 0;
    const imports = /@import\s+(?:url\(\s*)?["']([^"']+)["']\s*\)?\s*([^;]*);/gi;
    for (const match of source.matchAll(imports)) {
      result += source.slice(cursor, match.index);
      const path = localPath(match[1]!, base);
      const data = path ? await asset(path) : null;
      const text = data ? decodeText(data) : null;
      if (path && text !== null) {
        const imported = await css(text, path.slice(0, path.lastIndexOf('/') + 1), depth + 1);
        result += match[2]?.trim() ? `@media ${match[2]}{${imported}}` : imported;
      }
      cursor = match.index + match[0].length;
    }
    result += source.slice(cursor);
    const urls = [...result.matchAll(/url\(\s*(?:"([^"]*)"|'([^']*)'|([^\s)]*))\s*\)/gi)];
    for (const match of urls.reverse()) {
      const value = match[1] ?? match[2] ?? match[3] ?? '';
      const path = localPath(value, base);
      if (!path) continue;
      const data = await asset(path);
      result = result.slice(0, match.index) + `url("${data ?? ''}")` + result.slice(match.index + match[0].length);
    }
    return result;
  }
  async function visit(node: Node): Promise<void> {
    if ('tagName' in node) {
      const element: Element = node;
      if (element.tagName === 'base') { element.attrs = []; return; }
      const get = (name: string) => element.attrs.find(attr => attr.name === name);
      // Files must not weaken the sandbox's CSP or refresh to a new document.
      if (element.tagName === 'meta' && /^(?:content-security-policy|refresh)$/i.test(get('http-equiv')?.value ?? '')) {
        element.attrs = []; return;
      }
      const style = get('style');
      if (style) style.value = await css(style.value, '');
      if (element.tagName === 'style') {
        for (const child of element.childNodes) if ('value' in child) child.value = await css(child.value, '');
      }
      for (const name of ['src', 'poster', 'href']) {
        const attribute = get(name);
        if (!attribute || (name === 'href' && element.tagName !== 'link')) continue;
        const path = localPath(attribute.value);
        if (!path) continue;
        let data = await asset(path);
        if (data && element.tagName === 'link' && /stylesheet/i.test(get('rel')?.value ?? '')) {
          const text = decodeText(data);
          data = text === null ? null : textDataUrl(await css(text, path.slice(0, path.lastIndexOf('/') + 1)));
        }
        attribute.value = data ?? 'data:,';
      }
      // Avoid a browser resolving srcset candidates against the application URL.
      const srcset = get('srcset');
      if (srcset) {
        const candidates: string[] = [];
        for (const candidate of srcset.value.split(',')) {
          const [url, descriptor] = candidate.trim().split(/\s+/, 2);
          const path = url ? localPath(url) : null;
          const data = path ? await asset(path) : null;
          if (data) candidates.push(`${data}${descriptor ? ` ${descriptor}` : ''}`);
        }
        srcset.value = candidates.join(', ');
      }
    }
    if ('childNodes' in node) for (const child of node.childNodes) await visit(child);
    if ('content' in node) await visit(node.content);
  }
  await visit(document);
  return serialize(document);
}
