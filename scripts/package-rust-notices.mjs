import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { auditDependencies } from './rust-package-audit.mjs';
import { filesUnder } from './rust-package-common.mjs';

export function packageNotices(resources, target, packages = ['shellfox-desktop', 'shellfox-cli']) {
  const output = path.join(resources, 'THIRD-PARTY-NOTICES.txt');
  const audit = auditDependencies({ packages, target, output: resources,
    strict: true, allowStandardTexts: true });
  const sections = [readFileSync(output, 'utf8')];
  const seen = new Map();
  for (const dep of audit.dependencies) {
    if (dep.license?.includes('MPL-2.0') && dep.source?.startsWith('registry+')) {
      sections.push(`${dep.name} ${dep.version} source: https://crates.io/crates/${dep.name}/${dep.version}`);
    }
    for (const file of dep.licenseFiles) {
      if (file.role !== 'license-text') continue;
      const firstPath = seen.get(file.sha256);
      if (firstPath) sections.push(`===== ${file.path} (same license text as ${firstPath}) =====`);
      else {
        sections.push(`===== ${file.path} =====\n${readFileSync(path.join(resources, file.path), 'utf8').trim()}`);
        seen.set(file.sha256, file.path);
      }
    }
  }
  const ghostty = 'notices/ghostty-vt';
  const windows = 'elevated-terminal';
  for (const file of [
    'tailscale/THIRD-PARTY-NOTICES.txt', 'tailscale/TAILSCALE-LICENSE',
    ...filesUnder(path.join(resources, ghostty)).map(name => `${ghostty}/${name}`),
    'notices/conpty/LICENSE',
    `${windows}/THIRD-PARTY-NOTICES.TXT`, `${windows}/LICENSE.TXT`,
  ]) {
    const full = path.join(resources, file);
    if (existsSync(full)) sections.push(`===== ${file} =====\n${readFileSync(full, 'utf8').trim()}`);
  }
  writeFileSync(output, sections.join('\n\n') + '\n');
}
