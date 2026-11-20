import { describe, expect, it, vi } from 'vitest';
import { desktopPreviewUrl } from '../shared/file-preview-url';
import { permitPreviewNavigation } from './desktop-security';

vi.mock('electron', () => ({ protocol: {}, session: {} }));

const renderer = { processId: 1, routingId: 1 };
const preview = { processId: 2, routingId: 1 };
const document = desktopPreviewUrl('<p>Fixture preview</p>');

describe('desktop preview frame navigation', () => {
  it('permits the renderer to mount or replace a preview document', () => {
    expect(permitPreviewNavigation(document, { ...renderer }, renderer)).toBe(true);
    expect(permitPreviewNavigation(document, null, renderer)).toBe(true);
  });

  it('blocks preview self-navigation before the current document is replaced', () => {
    expect(permitPreviewNavigation(document, preview, renderer)).toBe(false);
    expect(permitPreviewNavigation(document, { processId: 1, routingId: 2 }, renderer)).toBe(false);
    for (const url of ['http://127.0.0.1:1234/', 'about:blank', 'file:///tmp/private.txt', 'shellfox-preview://other/?html=fixture']) {
      expect(permitPreviewNavigation(url, preview, renderer)).toBe(false);
      expect(permitPreviewNavigation(url, renderer, renderer)).toBe(false);
    }
  });
});
