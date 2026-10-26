import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
export function sourceDigest(root) {
  const entries = [];
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(file);
      else if (entry.isFile()) entries.push([path.relative(root, file).replaceAll('\\', '/'), createHash('sha256').update(readFileSync(file)).digest('hex')]);
    }
  }
  visit(path.join(root, 'src'));
  visit(path.join(root, 'native/Shellfox.ElevatedTerminal'));
  visit(path.join(root, 'native/tailscale'));
  for (const name of ['scripts/terminal-native.mjs', 'scripts/tailscale.mjs', 'scripts/build.mjs', 'scripts/forge.mjs', 'scripts/shellfox-cli-build.mjs', 'scripts/ssh-runtime.mjs', 'scripts/node-runtime.mjs', 'scripts/node-runtime-native.mjs', 'scripts/node-runtime-native-smoke.cjs', 'scripts/cli-host-smoke.mjs', 'scripts/desktop-cli-host-smoke.mjs', 'scripts/headless-smoke.mjs', 'scripts/headless-build.mjs', 'scripts/headless-package.mjs', 'scripts/headless-support.mjs', 'scripts/headless-service.mjs', 'scripts/source-digest.mjs', 'package.json', 'pnpm-lock.yaml', 'pnpm-workspace.yaml', '.npmrc', 'forge.config.cjs']) entries.push([name, createHash('sha256').update(readFileSync(path.join(root, name))).digest('hex')]);
  return createHash('sha256').update(JSON.stringify(entries)).digest('hex');
}
