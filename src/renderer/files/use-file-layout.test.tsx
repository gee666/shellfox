// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clampLayoutSize, FILE_BROWSER_HEIGHT_KEY, FILE_WIDTH_KEY, useFileLayout } from './use-file-layout';

const viewport = (width: number, height = 800) => {
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: width });
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: height });
  window.dispatchEvent(new Event('resize'));
};
beforeEach(() => { localStorage.clear(); viewport(1200); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); viewport(1024, 768); });

describe('file layout preferences', () => {
  it('clamps finite and invalid values, including containers smaller than the minimum', () => {
    expect(clampLayoutSize(100, 300, 700, 420)).toBe(300);
    expect(clampLayoutSize(900, 300, 700, 420)).toBe(700);
    expect(clampLayoutSize(NaN, 300, 700, 420)).toBe(420);
    expect(clampLayoutSize(420, 100, 100, 420)).toBe(100);
  });

  it('restores numeric preferences and ignores malformed or negative storage', () => {
    localStorage.setItem(FILE_WIDTH_KEY, '560'); localStorage.setItem(FILE_BROWSER_HEIGHT_KEY, '310');
    const first = renderHook(() => useFileLayout());
    expect(first.result.current.width).toBe(560); expect(first.result.current.browserHeight).toBe(310);
    first.unmount();
    localStorage.setItem(FILE_WIDTH_KEY, 'Infinity'); localStorage.setItem(FILE_BROWSER_HEIGHT_KEY, '-7');
    const next = renderHook(() => useFileLayout());
    expect(next.result.current.width).toBe(420); expect(next.result.current.browserHeight).toBe(220);
  });

  it('persists preferences independently of maximize and temporary responsive bounds', () => {
    const { result } = renderHook(() => useFileLayout());
    act(() => { result.current.setWidth(600); result.current.setBrowserHeight(350); result.current.toggleMaximized(); });
    expect(result.current.maximized).toBe(true);
    expect(localStorage.getItem(FILE_WIDTH_KEY)).toBe('600'); expect(localStorage.getItem(FILE_BROWSER_HEIGHT_KEY)).toBe('350');
    act(() => result.current.toggleMaximized());
    expect(result.current.width).toBe(600); expect(result.current.browserHeight).toBe(350);
  });

  it('survives disabled local storage', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('disabled'); });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('disabled'); });
    const { result } = renderHook(() => useFileLayout());
    act(() => result.current.setWidth(450)); expect(result.current.width).toBe(450);
  });
});
