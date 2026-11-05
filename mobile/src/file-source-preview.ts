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

const languages: Record<string, string> = {
  js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'jsx', ts: 'typescript', tsx: 'tsx',
  json: 'json', html: 'markup', htm: 'markup', xml: 'markup', svg: 'markup', css: 'css',
  sh: 'bash', bash: 'bash', zsh: 'bash', py: 'python', ps1: 'powershell', psm1: 'powershell',
  cs: 'csharp', md: 'markdown', markdown: 'markdown', mdx: 'markdown', yml: 'yaml', yaml: 'yaml',
};
export function fileSourceLanguage(path: string): string | null {
  return languages[path.split('.').at(-1)?.toLowerCase() ?? ''] ?? null;
}

// Prism escapes source text before inserting token spans. No file content becomes
// executable markup, and the containing WebView disables JavaScript as well.
export function fileSourcePreview(text: string, path: string, wrapLines = true): string {
  const language = fileSourceLanguage(path);
  const grammar = language ? Prism.languages[language] : undefined;
  const plain = () => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const highlighted = grammar && language && text.length <= 200_000 && !text.split('\n').some(line => line.length > 10_000)
    ? Prism.highlight(text, grammar, language) : plain();
  return `<style>pre{margin:0!important;background:transparent!important;padding:0!important;tab-size:2;overflow:auto}pre code{font:13px/1.5 Menlo,Consolas,monospace;white-space:${wrapLines ? 'pre-wrap' : 'pre'};overflow-wrap:${wrapLines ? 'anywhere' : 'normal'};word-break:normal}.token.comment,.token.prolog,.token.doctype,.token.cdata{color:#89969f}.token.punctuation{color:#aebbc5}.token.tag,.token.property,.token.boolean,.token.number,.token.constant,.token.symbol,.token.deleted{color:#ed9e84}.token.selector,.token.attr-name,.token.string,.token.char,.token.builtin,.token.inserted{color:#92c894}.token.operator,.token.entity,.token.url{color:#d4b67e}.token.atrule,.token.attr-value,.token.keyword{color:#a8aff0}.token.function,.token.class-name{color:#6fc5d4}.token.regex,.token.important,.token.variable{color:#ecc370}</style><pre><code>${highlighted}</code></pre>`;
}
