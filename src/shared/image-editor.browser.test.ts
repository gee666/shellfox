import { chromium, type Page } from '@playwright/test';
import { expect, it } from 'vitest';
import { imageEditorDocument } from './image-editor-document';
import { desktopPreviewUrl } from './file-preview-url';
import { parseImageEditorMessage, type EditableImage, type ImageEditorMessage } from './image-editor-protocol';

const token = 'image_editor_browser_token';
async function openEditor(page: Page, mimeType: 'image/png' | 'image/jpeg' | 'image/webp' = 'image/png', padding = 0, noisy = false, fileActions = false) {
  let dataUrl = await page.evaluate(({ mime, noisy }) => {
    const canvas = document.createElement('canvas'); canvas.width = noisy ? 512 : 3; canvas.height = noisy ? 512 : 2;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Canvas unavailable');
    if (noisy) {
      const pixels = context.createImageData(512, 512), bytes = new Uint8Array(pixels.data.buffer);
      for (let offset = 0; offset < bytes.length; offset += 65536) crypto.getRandomValues(bytes.subarray(offset, offset + 65536));
      for (let offset = 3; offset < bytes.length; offset += 4) bytes[offset] = 255;
      context.putImageData(pixels, 0, 0);
    } else ['#f00', '#0f0', '#00f', '#fff', '#000', '#ff0'].forEach((color, index) => {
      context.fillStyle = color; context.fillRect(index % 3, Math.floor(index / 3), 1, 1);
    });
    return canvas.toDataURL(mime, 0.92);
  }, { mime: mimeType, noisy });
  let bytes = Buffer.from(dataUrl.slice(dataUrl.indexOf(',') + 1), 'base64');
  if (padding) { bytes = Buffer.concat([bytes, Buffer.alloc(padding)]); dataUrl = `data:${mimeType};base64,${bytes.toString('base64')}`; }
  const image: EditableImage = { kind: 'image', path: '/photo.png', mimeType, size: bytes.length, revision: 'a'.repeat(64), dataUrl };
  const messages: ImageEditorMessage[] = [];
  page.on('console', message => {
    const prefix = 'image-editor-message:';
    if (!message.text().startsWith(prefix)) return;
    const parsed = parseImageEditorMessage(message.text().slice(prefix.length), token);
    if (parsed) messages.push(parsed);
  });
  await page.evaluate(() => {
    Object.defineProperty(window, 'ReactNativeWebView', { configurable: true, value: {
      postMessage: (data: string) => console.debug('image-editor-message:' + data),
    } });
  });
  const bootstrap = imageEditorDocument({ image, token, platform: 'mobile', fileActions });
  await page.setContent(bootstrap);
  await page.evaluate(message => window.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(message) })), { type: 'load', token, dataUrl });
  await page.locator('#status').filter({ hasText: noisy ? '512 × 512' : '3 × 2' }).waitFor();
  const command = (type: 'export' | 'unlock' | 'saved') => page.evaluate(message => {
    window.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(message) }));
  }, { type, token });
  const save = async () => {
    await page.getByRole('button', { name: 'Save image', exact: true }).click();
    await expect.poll(() => messages.at(-1)?.type).toBe('save-request');
    expect(await page.getByRole('button', { name: 'Save image', exact: true }).getAttribute('aria-busy')).toBe('true');
    await command('export'); await expect.poll(() => messages.at(-1)?.type).toBe('export');
    const exported = messages.at(-1); if (exported?.type !== 'export') throw new Error('No exported image');
    return exported.dataUrl;
  };
  return { image, bootstrap, messages, command, save };
}
async function pixels(page: Page) {
  return page.locator('#image').evaluate(element => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error('Missing canvas');
    const context = element.getContext('2d');
    if (!context) throw new Error('Canvas unavailable');
    const data = context.getImageData(0, 0, element.width, element.height).data;
    return { width: element.width, height: element.height, colors: Array.from({ length: data.length / 4 }, (_, index) =>
      Array.from(data.slice(index * 4, index * 4 + 3))) };
  });
}
it('edits actual pixels, crops a pointer selection and resets without a network connection', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    const requests: string[] = [];
    page.on('request', request => { if (/^(?:https?|file):/.test(request.url())) requests.push(request.url()); });
    await page.route('https://**/*', route => route.abort());
    const f = await openEditor(page);
    expect(await page.locator('#status').evaluate(element => getComputedStyle(element).color)).toBe('rgb(221, 221, 221)');
    expect(await page.locator('body').evaluate(element => getComputedStyle(element).backgroundColor)).toBe('rgb(23, 23, 27)');
    expect(await page.locator('#rotate-right').evaluate(element => getComputedStyle(element).backgroundColor)).toBe('rgb(36, 36, 41)');
    await page.getByRole('button', { name: 'Rotate right' }).click();
    expect(await pixels(page)).toEqual({ width: 2, height: 3, colors: [[255,255,255], [255,0,0], [0,0,0], [0,255,0], [255,255,0], [0,0,255]] });
    await page.getByRole('button', { name: 'Rotate left' }).click();
    await page.getByRole('button', { name: 'Flip horizontal' }).click();
    await page.getByRole('button', { name: 'Flip vertical' }).click();
    await page.locator('#image').evaluate(element => { if (element instanceof HTMLCanvasElement) { element.style.width = '300px'; element.style.height = '200px'; } });
    const area = await page.locator('#image').boundingBox(); if (!area) throw new Error('Missing canvas bounds');
    await page.mouse.move(area.x + 100, area.y); await page.mouse.down();
    await page.mouse.move(area.x + 300, area.y + 200, { steps: 4 }); await page.mouse.up();
    await page.getByRole('button', { name: 'Crop', exact: true }).click();
    expect(await pixels(page)).toEqual({ width: 2, height: 2, colors: [[0,0,0], [255,255,255], [0,255,0], [255,0,0]] });
    const exported = await f.save();
    const bytes = Buffer.from(exported.split(',')[1], 'base64');
    expect(bytes.subarray(0, 8).toString('hex')).toBe('89504e470d0a1a0a');
    expect([bytes.readUInt32BE(16), bytes.readUInt32BE(20)]).toEqual([2, 2]);
    await f.command('unlock'); await page.getByRole('button', { name: 'Reset', exact: true }).click();
    expect(await pixels(page)).toMatchObject({ width: 3, height: 2 });
    expect(await page.evaluate(async () => { try { await fetch('https://example.invalid/leak'); return false; } catch { return true; } })).toBe(true);
    expect(requests).toEqual([]);
  } finally { await browser.close(); }
}, 20_000);

it('selects a crop with a real touch drag', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 500, height: 800 }, hasTouch: true });
    const f = await openEditor(page);
    await page.locator('#image').evaluate(element => { if (element instanceof HTMLCanvasElement) { element.style.width = '300px'; element.style.height = '200px'; } });
    const box = await page.locator('#image').boundingBox();
    if (!box) throw new Error('Canvas not visible');
    const input = await page.context().newCDPSession(page);
    await input.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: box.x + 100, y: box.y + 2 }] });
    await input.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: box.x + 299, y: box.y + 199 }] });
    await input.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    expect(await page.getByRole('button', { name: 'Crop', exact: true }).isEnabled()).toBe(true);
    await page.getByRole('button', { name: 'Crop', exact: true }).click();
    expect(await pixels(page)).toMatchObject({ width: 2, height: 2 });
    const exported = await f.save(), bytes = Buffer.from(exported.split(',')[1], 'base64');
    expect([bytes.readUInt32BE(16), bytes.readUInt32BE(20)]).toEqual([2, 2]);
    await f.command('saved'); expect(await page.getByRole('button', { name: 'Save image', exact: true }).isDisabled()).toBe(true);
  } finally { await browser.close(); }
}, 20_000);

it('keeps all seven compact icon actions in one row with Save last on desktop and touch screens', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    for (const [width, touch] of [[240, false], [320, true], [360, true]] as const) {
      const page = await browser.newPage({ viewport: { width, height: 600 }, hasTouch: touch });
      await openEditor(page);
      const layout = await page.locator('.toolbar').evaluate(element => {
        const buttons = [...element.querySelectorAll('button')];
        return { count: buttons.length, names: buttons.map(button => button.getAttribute('aria-label')),
          top: buttons.map(button => button.getBoundingClientRect().top), width: buttons.map(button => button.getBoundingClientRect().width),
          text: buttons.map(button => button.textContent), fits: element.scrollWidth <= element.clientWidth };
      });
      expect(layout.count).toBe(7); expect(layout.names.at(-1)).toBe('Save image'); expect(new Set(layout.top).size).toBe(1);
      expect(layout.width).toEqual(new Array(7).fill(touch ? 32 : 26)); expect(layout.fits).toBe(true);
      expect(layout.text).toEqual(new Array(7).fill('')); expect(await page.locator('input,p').count()).toBe(0);
      await page.close();
    }
  } finally { await browser.close(); }
}, 20_000);

it('fits the mobile canvas below nine balanced actions and preserves pixels/selection through More and orientation changes', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    for (const [width, height] of [[320, 640], [360, 740], [680, 320]] as const) {
      const page = await browser.newPage({ viewport: { width, height }, hasTouch: true });
      const f = await openEditor(page, 'image/png', 0, false, true);
      const canvas = page.locator('#image'), originalNode = await canvas.elementHandle();
      if (!originalNode) throw new Error('Missing canvas');
      const dataUrl = await canvas.evaluate(element => { if (!(element instanceof HTMLCanvasElement)) throw new Error('Not a canvas'); return element.toDataURL(); });
      const layout = () => page.evaluate(() => {
        const image = document.querySelector('canvas'), viewport = document.getElementById('viewport'), stage = document.getElementById('stage'), status = document.getElementById('status');
        if (!image || !viewport || !stage || !status) throw new Error('Missing editor layout');
        const box = image.getBoundingClientRect(), available = viewport.getBoundingClientRect(), stageBox = stage.getBoundingClientRect();
        const buttons = [...document.querySelectorAll('.toolbar button')];
        return { names: buttons.map(button => button.getAttribute('aria-label')), top: buttons.map(button => button.getBoundingClientRect().top),
          buttonWidths: buttons.map(button => button.getBoundingClientRect().width), firstLeft: buttons[0].getBoundingClientRect().left,
          saveRight: buttons.at(-1)?.getBoundingClientRect().right ?? 0, canvas: { x: box.x, y: box.y, width: box.width, height: box.height, right: box.right, bottom: box.bottom },
          viewport: { x: available.x, y: available.y, width: available.width, height: available.height, right: available.right, bottom: available.bottom },
          stage: { x: stageBox.x, y: stageBox.y, width: stageBox.width, height: stageBox.height }, bitmap: [image.width, image.height],
          statusPosition: getComputedStyle(status).position, statusOpacity: getComputedStyle(status).opacity };
      });
      const size = await layout();
      expect(size.names).toEqual(['Close file', 'More file actions', 'Rotate left', 'Rotate right', 'Flip horizontal', 'Flip vertical', 'Crop', 'Reset', 'Save image']);
      expect(new Set(size.top).size).toBe(1); expect(size.buttonWidths).toEqual(new Array(9).fill(32));
      expect(size.firstLeft).toBe(6); expect(width - size.saveRight).toBe(6);
      expect(size.viewport.height).toBe(height - 40); expect(size.statusPosition).toBe('absolute'); expect(size.statusOpacity).toBe('0');
      expect(size.canvas.x).toBeGreaterThanOrEqual(size.viewport.x - 0.05); expect(size.canvas.y).toBeGreaterThanOrEqual(size.viewport.y - 0.05);
      expect(size.canvas.right).toBeLessThanOrEqual(size.viewport.right + 0.05); expect(size.canvas.bottom).toBeLessThanOrEqual(size.viewport.bottom + 0.05);
      expect(Math.min(Math.abs(size.canvas.width - size.viewport.width), Math.abs(size.canvas.height - size.viewport.height))).toBeLessThan(0.05);
      expect(size.bitmap).toEqual([3, 2]); expect(size.canvas.width / size.canvas.height).toBeCloseTo(1.5, 3);
      expect(size.stage).toEqual({ x: size.canvas.x, y: size.canvas.y, width: size.canvas.width, height: size.canvas.height });
      await page.mouse.move(size.canvas.x + size.canvas.width / 3, size.canvas.y + 1); await page.mouse.down();
      await page.mouse.move(size.canvas.right - 1, size.canvas.bottom - 1, { steps: 4 }); await page.mouse.up();
      await page.getByRole('button', { name: 'More file actions', exact: true }).click();
      await expect.poll(() => f.messages.at(-1)?.type).toBe('file-action');
      expect(f.messages.at(-1)).toEqual({ type: 'file-action', action: 'more', token });
      expect(await canvas.evaluate((element, previous) => element === previous, originalNode)).toBe(true);
      expect(await canvas.evaluate(element => element instanceof HTMLCanvasElement ? element.toDataURL() : '')).toBe(dataUrl);
      await page.setViewportSize({ width: 700, height: 320 });
      await expect.poll(async () => { const current = await layout(); return Math.abs(current.canvas.height - current.viewport.height); }).toBeLessThan(0.05);
      const selected = await page.locator('#selection').boundingBox(), resized = await canvas.boundingBox();
      if (!selected || !resized) throw new Error('Crop selection was lost');
      expect(selected.x - resized.x).toBeCloseTo(resized.width / 3, 1); expect(selected.width).toBeCloseTo(resized.width * 2 / 3, 1);
      expect(selected.height).toBeCloseTo(resized.height, 1);
      await page.getByRole('button', { name: 'Crop', exact: true }).click(); expect(await pixels(page)).toMatchObject({ width: 2, height: 2 });
      const output = await f.save(), bytes = Buffer.from(output.split(',')[1], 'base64');
      expect([bytes.readUInt32BE(16), bytes.readUInt32BE(20)]).toEqual([2, 2]); await f.command('saved');
      expect(await page.getByRole('button', { name: 'Close file', exact: true }).isEnabled()).toBe(true);
      await page.close();
    }
  } finally { await browser.close(); }
}, 20_000);

it('loads a valid >8 MiB raster through the handshake without growing the bootstrap URL', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); const f = await openEditor(page, 'image/png', 9 * 1024 * 1024);
    expect(f.image.size).toBeGreaterThan(8 * 1024 * 1024);
    expect(f.bootstrap.length).toBeLessThan(100_000); expect(desktopPreviewUrl(f.bootstrap).length).toBeLessThan(200_000);
    expect(f.bootstrap).not.toContain(f.image.dataUrl); expect(f.messages[0]?.type).toBe('request-image');
    await page.getByRole('button', { name: 'Rotate right', exact: true }).click(); expect(await pixels(page)).toMatchObject({ width: 2, height: 3 });
  } finally { await browser.close(); }
}, 20_000);

it('exports an actual PNG above the old 512 KiB write cap without losing pixels', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); const f = await openEditor(page, 'image/png', 0, true);
    expect(f.image.size).toBeGreaterThan(512 * 1024);
    await page.getByRole('button', { name: 'Rotate right', exact: true }).click();
    const output = await f.save(), bytes = Buffer.from(output.slice(output.indexOf(',') + 1), 'base64');
    expect(bytes.length).toBeGreaterThan(512 * 1024); expect([bytes.readUInt32BE(16), bytes.readUInt32BE(20)]).toEqual([512, 512]);
  } finally { await browser.close(); }
}, 20_000);

it.each(['image/jpeg', 'image/webp'] satisfies Array<EditableImage['mimeType']>)('exports valid %s bytes without changing format', async mimeType => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    const f = await openEditor(page, mimeType);
    await page.getByRole('button', { name: 'Rotate right' }).click();
    const exported = await f.save();
    expect(exported.startsWith(`data:${mimeType};base64,`)).toBe(true);
    const bytes = Buffer.from(exported.split(',')[1], 'base64');
    expect(mimeType === 'image/jpeg' ? bytes.subarray(0, 2).toString('hex') : bytes.subarray(0, 4).toString()).toBe(mimeType === 'image/jpeg' ? 'ffd8' : 'RIFF');
  } finally { await browser.close(); }
}, 20_000);
