import type { IParser } from '@xterm/xterm';

/** The backend's persistent VT parser answers these once, before any replay. */
export function installTerminalQueryHandlers(parser: IParser, historical: () => boolean) {
  const handlers = [
    parser.registerCsiHandler({ final: 'c' }, () => true),
    parser.registerCsiHandler({ prefix: '>', final: 'c' }, () => true),
    parser.registerCsiHandler({ final: 'n' }, params => params[0] === 5 || params[0] === 6),
    parser.registerCsiHandler({ prefix: '?', final: 'n' }, params => params[0] === 6),
    parser.registerCsiHandler({ intermediates: '$', final: 'p' }, () => true),
    parser.registerCsiHandler({ prefix: '?', intermediates: '$', final: 'p' }, () => true),
    parser.registerDcsHandler({ intermediates: '$', final: 'q' }, () => true),
    // Text-grid reports belong to the backend. Pixel reports need the live DOM.
    parser.registerCsiHandler({ final: 't' }, params => params[0] === 18 || historical() && (params[0] === 14 || params[0] === 16)),
    // Palette/theme reports require the browser's actual colors. Never answer
    // historical queries after the application that asked may have exited.
    // A mixed set/report OSC is consumed whole on replay, favoring safe input
    // over restoring that command's palette changes. Set-only OSCs pass through.
    ...[4, 10, 11, 12].map(id => parser.registerOscHandler(id, data => historical() && data.split(';').includes('?'))),
  ];
  return { dispose: () => handlers.forEach(handler => handler.dispose()) };
}
