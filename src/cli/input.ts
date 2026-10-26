import { open } from 'node:fs/promises';
import type { Readable } from 'node:stream';
import { CliUsageError } from './arguments';
import { readStdin } from './secrets';
const LIMIT = 1024 * 1024;
/** --input accepts inline JSON, '-' for stdin, or '@path' for a JSON file. */
export async function loadJsonArguments(args: string[], input: Readable = process.stdin): Promise<string[]> {
  const result = [...args];
  for (let i = 0; i < result.length; i++) {
    if (result[i] === '--') break;
    const inline = result[i].startsWith('--input=');
    if (!inline && result[i] !== '--input') continue;
    const index = inline ? i : ++i;
    const value = inline ? result[index].slice('--input='.length) : result[index];
    let text: string;
    if (value === '-') text = await readStdin(input, LIMIT);
    else if (value?.startsWith('@')) {
      if (value.length === 1) throw new CliUsageError('Missing JSON input file path.');
      const handle = await open(value.slice(1), 'r');
      try {
        const info = await handle.stat();
        if (!info.isFile() || info.size > LIMIT) throw new CliUsageError('JSON input must be a regular file no larger than 1 MiB.');
        const buffer = Buffer.alloc(LIMIT + 1);
        const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
        if (bytesRead > LIMIT) throw new CliUsageError('JSON input exceeds 1 MiB.');
        text = new TextDecoder('utf-8', { fatal: true }).decode(buffer.subarray(0, bytesRead));
      } finally { await handle.close(); }
    } else continue;
    result[index] = inline ? '--input=' + text : text;
  }
  return result;
}
