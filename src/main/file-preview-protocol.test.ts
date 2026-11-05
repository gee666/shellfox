import { describe, expect, it, vi } from 'vitest';
vi.mock('electron', () => ({ protocol: { registerSchemesAsPrivileged: vi.fn(), handle: vi.fn() } }));
import { protocol } from 'electron';
import { installFilePreviewProtocol, previewDocumentResponse, registerFilePreviewScheme } from './file-preview-protocol';
import { desktopPreviewUrl, FILE_PREVIEW_CSP, FILE_PREVIEW_URL_LIMIT } from '../shared/file-preview-url';

describe('isolated file preview documents', () => {
  it('serves document bytes with an opaque script-only sandbox, never a filesystem path', async () => {
    const html = '<script>document.title="interactive"</script><p>Unicode: ⛄</p>';
    const response = previewDocumentResponse({ url: desktopPreviewUrl(html), method: 'GET' });
    expect(response.status).toBe(200);
    expect(await response.text()).toBe(html);
    expect(response.headers.get('Content-Security-Policy')).toBe(FILE_PREVIEW_CSP);
    expect(FILE_PREVIEW_CSP).toContain('sandbox allow-scripts');
    expect(FILE_PREVIEW_CSP).not.toContain('allow-same-origin');
    expect(FILE_PREVIEW_CSP).toContain("connect-src 'none'");
    expect(response.headers.get('Cache-Control')).toBe('no-store');
  });
  it('rejects wrong methods, hosts, paths, credentials and oversized payloads', () => {
    for (const url of ['file:///etc/passwd', 'shellfox-preview://other/?html=x', 'shellfox-preview://document/etc/passwd?html=x', 'shellfox-preview://user@document/?html=x', 'shellfox-preview://document/']) {
      expect(previewDocumentResponse({ url, method: 'GET' }).status).toBeGreaterThanOrEqual(400);
    }
    expect(previewDocumentResponse({ url: desktopPreviewUrl('x'), method: 'POST' }).status).toBe(400);
    expect(() => desktopPreviewUrl('x'.repeat(FILE_PREVIEW_URL_LIMIT))).toThrow('size limit');
  });
  it('registers a distinct scheme without bypassing CSP or giving it fetch privileges', () => {
    registerFilePreviewScheme(); installFilePreviewProtocol();
    expect(protocol.registerSchemesAsPrivileged).toHaveBeenCalledWith([{ scheme: 'shellfox-preview', privileges: { standard: true, secure: true } }]);
    expect(protocol.handle).toHaveBeenCalledWith('shellfox-preview', expect.any(Function));
  });
});
