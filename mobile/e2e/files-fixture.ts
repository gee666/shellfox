// Shared deterministic host data for the isolated demo and explicit QA entries.
import type { FileApi, FileRead } from '../../src/shared/file-contracts';
import { failure, success } from '../../src/shared/contracts';
import { fixtureFileTransfers, transferFixtures, nativeLargeSize } from './FileTransferHarness';
import { nativeFixtureImage } from './image-fixture';

const revision = 'a'.repeat(64);
const image = nativeFixtureImage;
const icon = 'data:image/x-icon;base64,AAABAAEACAQAAAEAIABrAAAAFgAAAIlQTkcNChoKAAAADUlIRFIAAAAIAAAABAgGAAAAs81+8AAAACBJREFUeAGkyrENAAAMwrCIy/t5+QAGImWzOD4tSjswAAAA//9N8jKNAAAABklEQVQDAF/TDAFxcYBpAAAAAElFTkSuQmCC';
function dataSize(value: string) { const encoded = value.slice(value.indexOf(',') + 1); return encoded.length * 3 / 4 - (encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0); }
const imageSize = dataSize(image);
const text = (name: string, mimeType: string, value: string): FileRead => ({ kind: 'text', path: `/fixture/${name}`, mimeType, text: value, size: value.length, revision });
const seed: FileRead[] = [
  text('code.ts', 'text/plain', 'const answer: number = 42;\nconsole.log(answer);\n// This deliberately long source line shows the default soft wrap on a narrow phone without forcing sideways scrolling or hiding the end of the code behind the edge of the screen.\n'),
  text('subdir/nested.txt', 'text/plain', 'Nested fixture'),
  text('README.md', 'text/markdown', '# Native Markdown\n\n![Fixture image](image.png)\n\n**Markdown sibling loaded**'),
  text('index.html', 'text/html', '<h1>Native HTML</h1><button onclick="this.textContent=String(window.fixtureValue)">Run preview</button><script src="local.js"></script>'),
  text('icon.svg', 'image/svg+xml', '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="green"/></svg>'),
  { kind: 'image', path: '/fixture/image.png', mimeType: 'image/png', size: imageSize, revision, dataUrl: image },
  { kind: 'image', path: '/fixture/icon.ico', mimeType: 'image/x-icon', size: dataSize(icon), revision, dataUrl: icon },
];
export function createFilesFixture(report: (message: string) => void = () => {}, options: { qa?: boolean } = {}) {
  const directoryApi = { getHomeDirectory: async () => success({ cwd: '/fixture' }),
    completeDirectory: async ({ path }: { path: string }) => success({ matches: path.startsWith('/fixture/s') ? ['/fixture/subdir/'] : [] }),
  };
    const files = new Map([...seed, ...(options.qa ? [...transferFixtures, text('qa-slow.txt', 'text/plain', 'Delayed fixture read')] : [])].map(file => [file.path, { ...file }]));
    const api: FileApi = {
      ...fixtureFileTransfers(files, report, options),
      list: async input => {
        if ((input.sessionId || input.tabId) && (input.sessionId !== '10000000-0000-4000-8000-000000000001' || input.tabId !== '10000000-0000-4000-8000-000000000002')) return failure('ACCESS_DENIED', 'Wrong fixture session/tab scope.');
        if (!['.', '~', '/fixture', '/fixture/subdir'].includes(input.path)) return failure('ACCESS_DENIED', 'Outside fixture folder.');
        const folder = input.path === '.' || input.path === '~' ? '/fixture' : input.path;
        return success({ path: folder, parentPath: folder === '/fixture' ? null : '/fixture', truncated: false,
          entries: [...Array.from(files.values()).filter(file => file.path.slice(0, file.path.lastIndexOf('/')) === folder)
            .map(file => ({ name: file.path.slice(folder.length + 1), path: file.path, kind: 'file' as const })),
          ...(folder === '/fixture' ? [{ name: 'subdir', path: '/fixture/subdir', kind: 'directory' as const }] : [])] });
      },
      read: async input => {
        if (options.qa && input.path === '/fixture/qa-slow.txt') { await new Promise<void>(resolve => setTimeout(resolve, 8000)); report('Slow fixture read finished'); }
        if (input.path.endsWith('shellfox-transfer-large.bin')) return failure('UNSUPPORTED', `Fixture preview is too large: ${nativeLargeSize} bytes.`);
        const file = files.get(input.path);
        return file ? success(file) : failure('NOT_FOUND', 'Fixture file missing.');
      },
      write: async input => {
        const current = files.get(input.path);
        if (!current || current.kind !== 'text') return failure('NOT_FOUND', 'Fixture text missing.');
        if (input.revision !== current.revision) return failure('FILE_CONFLICT', 'Fixture changed.');
        const next = { ...current, text: input.text, size: input.text.length, revision: 'b'.repeat(64) };
        files.set(input.path, next); report(`Saved text: ${input.text.trim()}`);
        return success({ path: next.path, size: next.size, revision: next.revision });
      },
      readAsset: async input => {
        report(`Loaded asset: ${input.assetPath}`);
        return input.assetPath === 'image.png'
          ? success({ mimeType: 'image/png', size: imageSize, dataUrl: image })
          : input.assetPath === 'local.js' ? success({ mimeType: 'text/javascript', size: 24, dataUrl: 'data:text/javascript;base64,d2luZG93LmZpeHR1cmVWYWx1ZT0xMjM7' })
            : failure('NOT_FOUND', 'Fixture asset missing.');
      },
      writeImage: async input => {
        const current = files.get(input.path);
        if (!current || current.kind !== 'image') return failure('NOT_FOUND', 'Fixture image missing.');
        if (current.revision !== input.revision) return failure('FILE_CONFLICT', 'Fixture image changed.');
        const next = { ...current, revision: 'c'.repeat(64), dataUrl: input.dataUrl, size: dataSize(input.dataUrl) };
        files.set(input.path, next); report('Saved image');
        return success({ path: next.path, revision: next.revision, size: next.size });
      },
    };
  return { api, directoryApi };
}
