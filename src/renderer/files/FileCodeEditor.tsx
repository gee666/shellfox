import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import Prism from 'prismjs';
import 'prismjs/components/prism-markup';
import 'prismjs/components/prism-css';
import 'prismjs/components/prism-clike';
import 'prismjs/components/prism-javascript';
import 'prismjs/components/prism-typescript';
import 'prismjs/components/prism-jsx';
import 'prismjs/components/prism-tsx';
import 'prismjs/components/prism-json';
import 'prismjs/components/prism-bash';
import 'prismjs/components/prism-python';
import 'prismjs/components/prism-powershell';
import 'prismjs/components/prism-csharp';
import 'prismjs/components/prism-markdown';
import 'prismjs/components/prism-yaml';
import { sourcePosition } from './file-preview-utils';
import type { FileOpenRequest } from './use-file-document';

const languages: Record<string, string> = {
  js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'jsx', ts: 'typescript', tsx: 'tsx',
  json: 'json', html: 'markup', htm: 'markup', xml: 'markup', svg: 'markup', css: 'css',
  sh: 'bash', bash: 'bash', zsh: 'bash', py: 'python', ps1: 'powershell', psm1: 'powershell',
  cs: 'csharp', md: 'markdown', markdown: 'markdown', mdx: 'markdown', yml: 'yaml', yaml: 'yaml',
};
function highlightedSource(text: string, path: string): string | null {
  // Keep large or minified files editable without blocking the renderer on tokenization.
  if (text.length > 200_000 || text.split('\n').some(line => line.length > 10_000)) return null;
  const language = languages[path.split('.').at(-1)?.toLowerCase() ?? ''];
  const grammar = language ? Prism.languages[language] : undefined;
  return grammar ? Prism.highlight(text, grammar, language) : null;
}

export function FileCodeEditor({ path, text, request, onChange, onSave }: {
  path: string; text: string; request: FileOpenRequest | null; onChange: (text: string) => void; onSave: () => void;
}) {
  const [wrap, setWrap] = useState(true);
  const input = useRef<HTMLTextAreaElement>(null);
  const code = useRef<HTMLPreElement>(null);
  const html = useMemo(() => highlightedSource(text, path), [text, path]);
  const syncScroll = () => {
    if (!input.current || !code.current) return;
    // Subtract scrollbar gutters without rounding fractional CSS pixels.
    const bounds = input.current.getBoundingClientRect();
    if (input.current.clientWidth) code.current.style.width = `${bounds.width - input.current.offsetWidth + input.current.clientWidth}px`;
    if (input.current.clientHeight) code.current.style.height = `${bounds.height - input.current.offsetHeight + input.current.clientHeight}px`;
    code.current.scrollTop = input.current.scrollTop;
    code.current.scrollLeft = input.current.scrollLeft;
  };
  useLayoutEffect(syncScroll, [text, wrap]);
  useEffect(() => {
    if (!input.current || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(syncScroll);
    observer.observe(input.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!request?.line || !input.current) return;
    const offset = sourcePosition(input.current.value, request.line, request.column);
    input.current.focus();
    input.current.setSelectionRange(offset, offset);
    syncScroll();
    let top = Math.max(0, request.line - 4) * 18;
    if (code.current) {
      const walker = document.createTreeWalker(code.current, NodeFilter.SHOW_TEXT);
      let remaining = offset, node = walker.nextNode();
      while (node && remaining > (node.textContent?.length ?? 0)) {
        remaining -= node.textContent?.length ?? 0; node = walker.nextNode();
      }
      if (node) {
        const range = document.createRange(); range.setStart(node, remaining); range.collapse(true);
        if (typeof range.getBoundingClientRect === 'function') {
          top = range.getBoundingClientRect().top - code.current.getBoundingClientRect().top + code.current.scrollTop - 54;
        }
      }
    }
    input.current.scrollTop = Math.max(0, top); syncScroll();
  }, [request?.id]);
  return <div className="file-source-editor">
    <div className="file-code-toolbar"><button type="button" aria-pressed={wrap}
      title={wrap ? 'Disable line wrapping' : 'Wrap long lines'} onClick={() => setWrap(value => !value)}>{wrap ? 'Unwrap' : 'Wrap'}</button></div>
    <div className={`file-code-editor${html === null ? ' file-code-plain' : ''}${wrap ? ' file-code-wrap' : ''}`}>
    <pre ref={code} aria-hidden="true">{html === null ? text + '\n' : <code dangerouslySetInnerHTML={{ __html: html + '\n' }} />}</pre>
    <textarea ref={input} aria-label="File contents" value={text} wrap={wrap ? 'soft' : 'off'} spellCheck={false} autoCapitalize="off" autoCorrect="off"
      onChange={event => onChange(event.target.value)} onScroll={syncScroll}
      onKeyDown={event => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') { event.preventDefault(); onSave(); }
      }} />
    </div>
  </div>;
}
