// Test-owned endpoint for the native picker fixture. No network or host filesystem access.
import { failure, success } from '../../src/shared/contracts';
import type { TerminalUploadApi } from '../src/terminal/upload';

const uploadId = '12345678-1234-4234-8234-123456789abe';
const name = 'shellfox-attachment.txt';
const contents = 'Shellfox attachment fixture 0123456789\n'.repeat(6000);
export const fixtureAttachmentPath = '/home/mobile/uploads/mobile attachment.txt';

function base64(text: string): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  let output = '';
  for (let i = 0; i < text.length; i += 3) {
    const a = text.charCodeAt(i), b = text.charCodeAt(i + 1), c = text.charCodeAt(i + 2);
    output += alphabet[a >> 2] + alphabet[((a & 3) << 4) | (Number.isNaN(b) ? 0 : b >> 4)];
    output += Number.isNaN(b) ? '=' : alphabet[((b & 15) << 2) | (Number.isNaN(c) ? 0 : c >> 6)];
    output += Number.isNaN(c) ? '=' : alphabet[c & 63];
  }
  return output;
}

export function fixtureUploads(report: (status: string) => void): TerminalUploadApi {
  let offset = 0, chunks = 0, active = false;
  return {
    beginTerminalUpload: async input => {
      if (input.fileName !== name || input.size !== contents.length) return failure('VALIDATION', 'Choose the test-owned attachment fixture.');
      offset = 0; chunks = 0; active = true;
      report('Upload started');
      return success({ uploadId, chunkBytes: 196608 });
    },
    writeTerminalUpload: async input => {
      if (!active || input.uploadId !== uploadId || input.offset !== offset) return failure('VALIDATION', 'Invalid fixture offset.');
      const expected = contents.slice(offset, offset + 196608);
      if (input.data !== base64(expected)) return failure('VALIDATION', 'Native attachment bytes changed.');
      offset += expected.length; chunks++;
      return success({ offset });
    },
    finishTerminalUpload: async input => {
      if (!active || input.uploadId !== uploadId || offset !== contents.length) return failure('VALIDATION', 'Incomplete attachment fixture.');
      active = false;
      report(`Uploaded ${name}; bytes ${offset}; chunks ${chunks}; exact`);
      return success({ path: fixtureAttachmentPath, fileName: name, size: offset });
    },
    cancelTerminalUpload: async () => {
      active = false; report('Upload cancelled');
      return success({ cancelled: true });
    },
  };
}
