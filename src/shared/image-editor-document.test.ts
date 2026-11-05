// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { imageEditorDocument } from './image-editor-document';
import { IMAGE_EDITOR_SCRIPT } from './image-editor-script';
import { parseImageEditorMessage, type EditableImage, type ImageEditorOperation } from './image-editor-protocol';
import { FILE_IMAGE_WRITE_BYTES } from './file-schemas';

const token = 'image_editor_test_token';
const outputMimes: Array<EditableImage['mimeType']> = ['image/jpeg', 'image/webp'];
function png(width: number, height: number): EditableImage {
  const header = Buffer.alloc(24); Buffer.from('89504e470d0a1a0a', 'hex').copy(header);
  header.write('IHDR', 12); header.writeUInt32BE(width, 16); header.writeUInt32BE(height, 20);
  return { kind: 'image', path: '/photo.png', mimeType: 'image/png', revision: 'a'.repeat(64), size: header.length, dataUrl: `data:image/png;base64,${header.toString('base64')}` };
}
function setup(image = png(3, 2), operations: ImageEditorOperation[] = [], platform: 'desktop' | 'mobile' = 'mobile') {
  const messages: unknown[] = [];
  const send = vi.fn((value: string) => { messages.push(JSON.parse(value)); });
  const context = { drawImage: vi.fn(), translate: vi.fn(), rotate: vi.fn(), scale: vi.fn(), fillRect: vi.fn(), fillStyle: '' };
  const getContext = Object.getOwnPropertyDescriptor(HTMLCanvasElement.prototype, 'getContext');
  Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', { configurable: true, value: () => context });
  const toDataURL = vi.spyOn(HTMLCanvasElement.prototype, 'toDataURL').mockImplementation(mime => `data:${mime};base64,AAAA`);
  const imageCreated = vi.fn();
  class TestImage {
    naturalWidth = 3; naturalHeight = 2; onload: (() => void) | null = null;
    constructor() { imageCreated(); }
    set src(_value: string) { this.onload?.(); }
  }
  vi.stubGlobal('Image', TestImage);
  vi.stubGlobal('ReactNativeWebView', { postMessage: send });
  if (platform === 'desktop') vi.spyOn(window, 'postMessage').mockImplementation(value => { messages.push(value); });
  document.documentElement.innerHTML = imageEditorDocument({ image, token, platform, operations, parentOrigin: window.location.origin });
  const windowListeners = vi.spyOn(window, 'addEventListener');
  const documentListeners = vi.spyOn(document, 'addEventListener');
  window.eval(IMAGE_EDITOR_SCRIPT);
  const canvas = document.getElementById('image');
  if (!(canvas instanceof HTMLCanvasElement)) throw new Error('Missing canvas');
  const command = (type: string, commandToken = token, origin = window.location.origin, source: Window | null = window) => {
    window.dispatchEvent(new MessageEvent('message', { data: JSON.stringify({ type, token: commandToken }), origin, source }));
  };
  const click = (id: string) => { const button = document.getElementById(id); if (!(button instanceof HTMLButtonElement)) throw new Error(id); button.click(); };
  window.dispatchEvent(new MessageEvent('message', { data: JSON.stringify({ type: 'load', token, dataUrl: image.dataUrl }), origin: window.location.origin, source: window }));
  const state = () => messages.map(value => parseImageEditorMessage(value, token)).filter(value => value?.type === 'state').at(-1);
  Object.defineProperty(canvas, 'setPointerCapture', { configurable: true, value: vi.fn() });
  Object.defineProperty(canvas, 'hasPointerCapture', { configurable: true, value: () => true });
  Object.defineProperty(canvas, 'releasePointerCapture', { configurable: true, value: vi.fn() });
  const pointer = (type: string, x: number, y: number, id = 1) => {
    const event = new MouseEvent(type, { clientX: x, clientY: y, button: 0, bubbles: true });
    Object.defineProperty(event, 'isPrimary', { value: id === 1 }); Object.defineProperty(event, 'pointerId', { value: id });
    canvas.dispatchEvent(event);
  };
  const drag = (x1: number, y1: number, x2: number, y2: number) => {
    vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 300, 200));
    pointer('pointerdown', x1, y1); pointer('pointermove', x2, y2); pointer('pointerup', x2, y2);
  };
  return { canvas, pointer, drag, messages, context, toDataURL, imageCreated, command, click, state, restore: () => {
    for (const [type, listener, options] of windowListeners.mock.calls) window.removeEventListener(type, listener, options);
    for (const [type, listener, options] of documentListeners.mock.calls) document.removeEventListener(type, listener, options);
    if (getContext) Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', getContext);
  } };
}
let restore: (() => void) | undefined;
afterEach(() => { restore?.(); vi.restoreAllMocks(); vi.unstubAllGlobals(); document.documentElement.innerHTML = ''; });
function editor(...args: Parameters<typeof setup>) { const result = setup(...args); restore = result.restore; return result; }

describe('trusted canvas document', () => {
  it('uses nonce scripts and a no-network CSP with no user markup', () => {
    const html = imageEditorDocument({ image: { ...png(3, 2), path: '</script><script>attack()</script>' }, token, platform: 'mobile', parentOrigin: '</script><script>attack()</script>' });
    document.documentElement.innerHTML = html;
    expect(document.querySelectorAll('script')).toHaveLength(2);
    expect(getComputedStyle(document.documentElement).color).toBe('rgb(221, 221, 221)');
    expect(getComputedStyle(document.documentElement).backgroundColor).toBe('rgb(23, 23, 27)');
    expect(getComputedStyle(document.getElementById('crop')!).backgroundColor).toBe('rgb(36, 36, 41)');
    const policy = document.querySelector('meta[http-equiv]')?.getAttribute('content');
    for (const directive of ["default-src 'none'", `script-src 'nonce-${token}'`, "connect-src 'none'", "frame-src 'none'", "form-action 'none'", "base-uri 'none'"]) expect(policy).toContain(directive);
    expect(document.getElementById('configuration')?.textContent).not.toContain('</script>');
    expect(() => imageEditorDocument({ image: png(3, 2), token: '" bad', platform: 'mobile' })).toThrow();
  });
  it('rotates both ways, flips both axes and resets to the original with clean state', () => {
    const f = editor();
    expect(f.state()).toMatchObject({ dirty: false, width: 3, height: 2, operations: [] });
    f.click('rotate-right'); expect([f.canvas.width, f.canvas.height]).toEqual([2, 3]);
    expect(f.context.rotate).toHaveBeenLastCalledWith(Math.PI / 2);
    f.click('rotate-left'); expect([f.canvas.width, f.canvas.height]).toEqual([3, 2]);
    expect(f.context.rotate).toHaveBeenLastCalledWith(-Math.PI / 2);
    f.click('flip-horizontal'); expect(f.context.scale).toHaveBeenLastCalledWith(-1, 1);
    f.click('flip-vertical'); expect(f.context.scale).toHaveBeenLastCalledWith(1, -1);
    expect(f.state()).toMatchObject({ dirty: true, operations: new Array(4).fill(expect.any(Object)) });
    f.click('reset'); expect(f.state()).toMatchObject({ dirty: false, width: 3, height: 2, operations: [] });
  });
  it('crops only a nonempty pointer selection and replays the committed crop with its draft', () => {
    const f = editor(png(3, 2), [{ type: 'rotate', direction: 'right' }]);
    expect(document.getElementById('crop')).toHaveProperty('disabled', true);
    f.drag(100, 100, 100, 100); expect(document.getElementById('crop')).toHaveProperty('disabled', true);
    f.drag(150, 0, 300, 200); f.click('crop');
    expect(f.state()).toMatchObject({ width: 1, height: 3, operations: [{ type: 'rotate', direction: 'right' }, { type: 'crop', x: 1, y: 0, width: 1, height: 3 }] });
    f.click('reset'); expect([f.canvas.width, f.canvas.height]).toEqual([3, 2]);
  });
  it('has one compact SVG-only toolbar with Save last and synchronizes dirty/pending/saved controls', () => {
    const f = editor();
    const buttons = [...document.querySelectorAll('button')];
    expect(buttons.map(button => button.id)).toEqual(['rotate-left', 'rotate-right', 'flip-horizontal', 'flip-vertical', 'crop', 'reset', 'save']);
    expect(document.querySelectorAll('.toolbar')).toHaveLength(1); expect(document.querySelectorAll('input')).toHaveLength(0);
    for (const button of buttons) {
      expect(button.textContent).toBe(''); expect(button.querySelector('svg')).not.toBeNull(); expect(button.getAttribute('aria-label')).toBe(button.title);
    }
    expect(document.getElementById('save')).toHaveProperty('disabled', true);
    f.click('flip-horizontal'); expect(document.getElementById('save')).toHaveProperty('disabled', false);
    f.click('save'); f.click('save');
    expect(f.messages.filter(message => parseImageEditorMessage(message, token)?.type === 'save-request')).toHaveLength(1);
    expect(document.getElementById('save')?.getAttribute('aria-busy')).toBe('true');
    expect(document.getElementById('rotate-left')).toHaveProperty('disabled', true);
    f.command('export'); expect(f.messages.at(-1)).toMatchObject({ type: 'export' });
    f.command('unlock'); expect(document.getElementById('save')).toHaveProperty('disabled', false);
    f.click('save'); f.command('export'); f.command('saved');
    expect(document.getElementById('save')).toHaveProperty('disabled', true); expect(document.getElementById('save')?.getAttribute('aria-busy')).toBe('false');
  });
  it('locks controls during export and uses the original mime without a silent PNG fallback', () => {
    const f = editor(); f.click('rotate-right'); f.command('export');
    expect(f.toDataURL).toHaveBeenCalledWith('image/png', 0.92);
    expect(f.messages.at(-1)).toMatchObject({ type: 'export', dataUrl: 'data:image/png;base64,AAAA' });
    expect(document.getElementById('reset')).toHaveProperty('disabled', true);
    f.command('export'); expect(f.toDataURL).toHaveBeenCalledTimes(1);
    f.command('unlock'); expect(document.getElementById('reset')).toHaveProperty('disabled', false);
    f.command('export'); f.command('saved');
    f.click('flip-horizontal'); f.click('reset');
    expect(f.state()).toMatchObject({ dirty: false, width: 2, height: 3 });
  });
  it('validates desktop command source, origin and token before encoding', () => {
    const f = editor(png(3, 2), [], 'desktop'); f.click('rotate-right');
    f.command('export', 'wrong_editor_token'); f.command('export', token, 'https://evil.example');
    f.command('export', token, window.location.origin, null);
    expect(f.toDataURL).not.toHaveBeenCalled();
    f.command('export'); expect(f.toDataURL).toHaveBeenCalledOnce();
  });
  it('rejects image dimensions before creating a decoder', () => {
    const f = editor(png(8192, 8192));
    expect(f.imageCreated).not.toHaveBeenCalled();
    expect(f.messages.at(-1)).toMatchObject({ type: 'error', message: expect.stringContaining('16-megapixel') });
  });
  it.each(outputMimes)('encodes %s in its original format and rejects encoder fallback', mimeType => {
    const header = mimeType === 'image/jpeg' ? Buffer.from('ffd8ffc0000b080002000301011100ffd9', 'hex') : Buffer.alloc(30);
    if (mimeType === 'image/webp') { header.write('RIFF'); header.write('WEBPVP8X', 8); header[24] = 2; header[27] = 1; }
    const image = { ...png(3, 2), mimeType, dataUrl: `data:${mimeType};base64,${header.toString('base64')}` };
    const f = editor(image); f.click('rotate-right'); f.command('export');
    expect(f.toDataURL).toHaveBeenCalledWith(mimeType, 0.92);
    expect(f.messages.at(-1)).toMatchObject({ type: 'export', dataUrl: `data:${mimeType};base64,AAAA` });
    if (mimeType === 'image/jpeg') expect(f.context.fillRect).toHaveBeenCalledWith(0, 0, 2, 3);
    f.command('unlock'); f.toDataURL.mockReturnValue('data:image/png;base64,AAAA'); f.command('export');
    expect(f.messages.at(-1)).toMatchObject({ type: 'error', message: expect.stringContaining('cannot encode') });
  });
  it('rejects oversized output without downscaling or losing the edited canvas', () => {
    const f = editor(); f.click('rotate-right');
    f.toDataURL.mockReturnValue(`data:image/png;base64,${Buffer.alloc(FILE_IMAGE_WRITE_BYTES + 1).toString('base64')}`); f.command('export');
    expect(f.messages.at(-1)).toMatchObject({ type: 'error', message: expect.stringContaining('save limit') });
    expect(document.getElementById('reset')).toHaveProperty('disabled', false);
    expect([f.canvas.width, f.canvas.height]).toEqual([2, 3]);
  });
  it('rejects animated WebP before allocating a decoder', () => {
    const header = Buffer.alloc(30); header.write('RIFF'); header.write('WEBPVP8X', 8); header[20] = 2;
    const f = editor({ ...png(3, 2), mimeType: 'image/webp', dataUrl: `data:image/webp;base64,${header.toString('base64')}` });
    expect(f.imageCreated).not.toHaveBeenCalled();
    expect(f.messages.at(-1)).toMatchObject({ type: 'error', message: 'Animated WebP editing is not supported.' });
  });
  it('maps a primary pointer drag from displayed coordinates to crop pixels', () => {
    const f = editor();
    f.drag(100, 0, 300, 200);
    f.click('crop'); expect(f.state()).toMatchObject({ width: 2, height: 2, operations: [{ type: 'crop', x: 1, y: 0, width: 2, height: 2 }] });
  });
  it('stops at the history limit rather than losing edits', () => {
    const f = editor(png(3, 2), new Array(256).fill({ type: 'flip', axis: 'horizontal' }));
    f.click('rotate-right'); expect([f.canvas.width, f.canvas.height]).toEqual([3, 2]);
    expect(document.getElementById('status')?.textContent).toContain('Save or reset');
    expect(f.state()).toMatchObject({ operations: new Array(256).fill({ type: 'flip', axis: 'horizontal' }) });
  });
});
