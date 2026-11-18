import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const reference = JSON.parse(readFileSync(new URL('../unicode-v6-reference.json', import.meta.url)));
const root = resolve(dirname(require.resolve('@xterm/xterm')), '..');
for (const [file, hash] of [['src/common/input/UnicodeV6.ts', reference.sourceSha256], ['src/common/services/UnicodeService.ts', reference.serviceSourceSha256]]) {
  if (createHash('sha256').update(readFileSync(resolve(root, file))).digest('hex') !== hash) throw new Error(`Unapproved xterm source: ${file}`);
}
const { Terminal } = require('@xterm/headless');
const term = new Terminal({ allowProposedApi: true });
try {
  // Test-only access to the pinned reference provider, never production code.
  const provider = term._core.unicodeService._activeProvider;
  if (provider.version !== reference.provider) throw new Error('Wrong reference width provider');
  const bytes = Buffer.alloc(reference.codepoints);
  const digest = () => createHash('sha256').update(bytes).digest('hex');
  for (let cp = 0; cp < bytes.length; cp++) bytes[cp] = provider.wcwidth(cp);
  if (digest() !== reference.widthSha256) throw new Error('Width reference changed');
  for (const [state, hash] of Object.entries(reference.propertiesSha256)) {
    for (let cp = 0; cp < bytes.length; cp++) bytes[cp] = provider.charProperties(cp, Number(state));
    if (digest() !== hash) throw new Error(`Join reference changed: ${state}`);
  }
  console.log('Pinned UnicodeV6 widths and all five reachable join-state transitions match the retained reference.');
} finally { term.dispose(); }
