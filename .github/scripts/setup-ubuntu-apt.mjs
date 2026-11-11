import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function validateMirror(value) {
  // Reject whitespace and URL normalization that could change apt's token syntax.
  if (typeof value !== 'string' || !/^https:\/\/[^\s\\?#]+$/.test(value)) {
    throw new Error('Ubuntu mirror must be an HTTPS archive origin/path without credentials, query or fragment.');
  }
  const url = new URL(value);
  if (!url.hostname || url.username || url.password) {
    throw new Error('Ubuntu mirror must be an HTTPS archive origin/path without credentials, query or fragment.');
  }
  return value.replace(/\/+$/, '');
}

// GitHub images use direct URIs and mirror+file indirection with one or three
// slashes. Replace the source URI so apt cannot select the failing Azure route.
const ubuntuURI = /(?<!\S)(?:https?:\/\/(?:(?:[a-z]{2}\.|azure\.)?archive\.ubuntu\.com\/ubuntu|security\.ubuntu\.com\/ubuntu|ports\.ubuntu\.com\/ubuntu-ports)|mirror\+file:\/(?:\/\/)?etc\/apt\/apt-mirrors(?:-security)?\.txt)\/?(?=\s|$)/g;

export function rewriteSources(source, format, mirror) {
  mirror = validateMirror(mirror);
  let inURIs = false;
  return source.split('\n').map(line => {
    if (format === '.sources') {
      if (!/^\s/.test(line)) inURIs = /^URIs:/i.test(line);
      if (!inURIs || /^\s*#/.test(line)) return line;
    } else if (format !== '.list' || !/^\s*deb(?:-src)?\s/.test(line)) {
      return line;
    }
    return line.replace(ubuntuURI, () => mirror);
  }).join('\n');
}

export const aptPolicy = `Acquire::Retries "3";
Acquire::http::Timeout "30";
Acquire::https::Timeout "30";
APT::Update::Error-Mode "any";
`;

export function describeSources(source, format) {
  let inURIs = false;
  return source.split('\n').filter(line => {
    if (format === '.list') return /^\s*deb(?:-src)?\s/.test(line);
    if (!/^\s/.test(line)) inURIs = /^URIs:/i.test(line);
    return /^(?:Types|URIs|Suites|Components|Architectures|Enabled|Signed-By):/i.test(line) ||
      (inURIs && /^\s+\S/.test(line) && !/^\s*#/.test(line));
  }).map(line => {
    line = line.replace(/\s+#.*$/, '');
    // Legacy bracket options can contain authentication settings. Keep only the
    // archive URI and suites; never dump apt auth files or arbitrary fields.
    if (format === '.list') line = line.replace(/^(\s*deb(?:-src)?\s+)\[[^\]]*\]\s*/, '$1');
    return line.replace(/\b[a-z][a-z0-9+.-]*:[^\s]+/gi, value => {
      try {
        const url = new URL(value);
        if (!url.username && !url.password && !url.search && !url.hash) return value;
        if (url.username || url.password) { url.username = 'redacted'; url.password = ''; }
        if (url.search) url.search = '?redacted';
        if (url.hash) url.hash = '#redacted';
        return url.href;
      } catch { return '[invalid URI omitted]'; }
    });
  }).join('\n');
}

export function configureUbuntuApt(directory, mirror, log = () => {}) {
  mirror = validateMirror(mirror);
  const sourceDirectory = path.join(directory, 'sources.list.d');
  const files = [path.join(directory, 'sources.list')];
  for (const name of readdirSync(sourceDirectory)) {
    if (/\.(?:sources|list)$/.test(name)) files.push(path.join(sourceDirectory, name));
  }
  let changed = 0;
  for (const file of files) {
    if (!existsSync(file)) continue;
    const original = readFileSync(file, 'utf8');
    const updated = rewriteSources(original, path.extname(file), mirror);
    if (updated !== original) {
      writeFileSync(file, updated);
      changed++;
    }
    log(`APT source ${file} [${updated !== original ? 'rewritten' : 'unchanged'}]:\n${describeSources(updated, path.extname(file))}`);
  }
  // Playwright invokes apt itself, so keep the same bounded retries and strict
  // index-update failures for browser, desktop and headless dependencies.
  writeFileSync(path.join(directory, 'apt.conf.d/80shellfox-archives'), aptPolicy);
  return changed;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const changed = configureUbuntuApt('/etc/apt', process.argv[2], console.log);
  console.log(`Configured signed Ubuntu archives: ${changed} source file(s) changed.`);
}
