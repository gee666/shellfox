import { chromium } from '@playwright/test';
import { expect, it } from 'vitest';
import { filePreviewDocument } from './file-preview-document';

it('decodes ICO through a static sandboxed WebView document', async () => {
  const icon = 'data:image/x-icon;base64,AAABAAEACAQAAAEAIABrAAAAFgAAAIlQTkcNChoKAAAADUlIRFIAAAAIAAAABAgGAAAAs81+8AAAACBJREFUeAGkyrENAAAMwrCIy/t5+QAGImWzOD4tSjswAAAA//9N8jKNAAAABklEQVQDAF/TDAFxcYBpAAAAAElFTkSuQmCC';
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.setContent(filePreviewDocument(icon, 'image', '#121212', '#eeeeee'));
    const frame = page.frames().find(value => value !== page.mainFrame());
    if (!frame) throw new Error('Missing static image frame.');
    await frame.waitForFunction(() => document.querySelector('img')?.naturalWidth === 8);
    expect(await frame.locator('img').evaluate(image => image.naturalHeight)).toBe(4);
    expect(await page.locator('iframe').getAttribute('sandbox')).toBe('');
  } finally { await browser.close(); }
}, 20_000);
