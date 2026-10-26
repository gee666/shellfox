import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { it } from 'node:test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
// Test inputs must be synthetic. Never retain a live or revoked credential as a fixture.
const patterns = [
  new RegExp('\\b' + ['tskey', '(?:auth|api|client|oauth)', '[A-Za-z0-9_-]+'].join('-'), 'g'),
  new RegExp('\\bgh' + '[pousr]_[A-Za-z0-9]{20,}', 'g'),
  new RegExp('\\bgithub' + '_pat_[A-Za-z0-9_]{20,}', 'g'),
  new RegExp('\\bsk' + '-(?:proj-|svcacct-)?[A-Za-z0-9_-]{20,}', 'g'),
  new RegExp('\\bAK' + 'IA[0-9A-Z]{16}', 'g'),
];
const textExtensions = new Set(['.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.go', '.cs', '.json', '.yaml', '.yml', '.md', '.txt', '.sh', '.ps1', '.env']);

it('recognizes credential-shaped inputs without treating bare prefixes as keys', () => {
  const fixture = ['tskey', 'auth', 'synthetic-secret'].join('-');
  assert.ok(new RegExp(patterns[0].source).test(fixture));
  assert.equal(new RegExp(patterns[0].source).test(['tskey', 'auth', '...'].join('-')), false);
});

it('does not commit API-key-shaped literals, including test fixtures', () => {
  const files = new Set(execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], {
    cwd: root, encoding: 'utf8', timeout: 10_000,
  }).split('\0').filter(Boolean));
  const findings = [];
  for (const file of files) {
    if (!textExtensions.has(path.extname(file)) || !existsSync(path.join(root, file))) continue;
    const content = readFileSync(path.join(root, file), 'utf8');
    for (const pattern of patterns) {
      pattern.lastIndex = 0;
      for (const match of content.matchAll(pattern)) {
        const line = content.slice(0, match.index).split('\n').length;
        findings.push(`${file}:${line}`);
      }
    }
  }
  // Report locations only, never the matched credential.
  assert.deepEqual(findings, [], `Remove credential-shaped literals from: ${findings.join(', ')}`);
});
