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

// GitHub images use both direct URIs and mirror+file indirection. Replace the
// source URI, not the mirror list, so apt cannot select the failing Azure route.
const ubuntuURI = /(?<!\S)(?:https?:\/\/(?:(?:[a-z]{2}\.|azure\.)?archive\.ubuntu\.com\/ubuntu|security\.ubuntu\.com\/ubuntu|ports\.ubuntu\.com\/ubuntu-ports)|mirror\+file:\/etc\/apt\/apt-mirrors(?:-security)?\.txt)\/?(?=\s|$)/g;

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

export function configureUbuntuApt(directory, mirror) {
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
  }
  // Playwright invokes apt itself, so keep the same bounded retries and strict
  // index-update failures for browser, desktop and headless dependencies.
  writeFileSync(path.join(directory, 'apt.conf.d/80shellfox-archives'), aptPolicy);
  return changed;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const changed = configureUbuntuApt('/etc/apt', process.argv[2]);
  console.log(`Configured signed Ubuntu archives: ${changed} source file(s) changed.`);
}
