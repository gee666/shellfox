import { EventEmitter } from 'node:events';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { parseWindowState, restoreBounds, trackWindowState, type Rect, type WindowState } from './window-state';

const good: WindowState = { x: 100, y: 80, width: 1000, height: 700, maximized: false };
const display: Rect = { x: 0, y: 0, width: 1920, height: 1080 };

describe('parseWindowState', () => {
  it('accepts a valid state', () => expect(parseWindowState(good)).toEqual(good));
  it('rejects garbage', () => {
    for (const v of [null, undefined, 5, 'x', [], {}]) expect(parseWindowState(v)).toBeNull();
  });
  it('rejects NaN, infinities and fractions', () => {
    for (const bad of [NaN, Infinity, 1.5, '10']) expect(parseWindowState({ ...good, x: bad })).toBeNull();
    expect(parseWindowState({ ...good, width: NaN })).toBeNull();
  });
  it('rejects too small or absurd sizes', () => {
    expect(parseWindowState({ ...good, width: 849 })).toBeNull();
    expect(parseWindowState({ ...good, height: 599 })).toBeNull();
    expect(parseWindowState({ ...good, width: 1e9 })).toBeNull();
    expect(parseWindowState({ ...good, x: 1e9 })).toBeNull();
  });
  it('rejects missing fields', () => {
    for (const k of Object.keys(good)) { const { [k as keyof WindowState]: _, ...rest } = good; expect(parseWindowState(rest)).toBeNull(); }
  });
  it('accepts minimum sizes and negative coordinates', () => {
    expect(parseWindowState({ ...good, x: -1920, y: -50, width: 850, height: 600 })).not.toBeNull();
  });
});

describe('restoreBounds', () => {
  it('returns defaults without saved state', () => {
    expect(restoreBounds(null, [display], display)).toEqual({ options: { width: 1220, height: 820 }, maximized: false });
  });
  it('keeps position on the same display', () => {
    expect(restoreBounds(good, [display], display)).toEqual({ options: { x: 100, y: 80, width: 1000, height: 700 }, maximized: false });
  });
  it('centers and clamps when the monitor is gone', () => {
    const saved = { ...good, x: 3000, y: 100, width: 2500, height: 1400 };
    const r = restoreBounds(saved, [{ x: 0, y: 0, width: 1366, height: 768 }], { x: 0, y: 0, width: 1366, height: 768 });
    expect(r.options).toEqual({ width: 1366, height: 768 });
    expect('x' in r.options).toBe(false);
  });
  it('never clamps below window minimums', () => {
    const r = restoreBounds({ ...good, x: 9000 }, [display], { x: 0, y: 0, width: 800, height: 500 });
    expect(r.options).toEqual({ width: 850, height: 600 });
  });
  it('keeps a partially visible title bar', () => {
    const saved = { ...good, x: 1920 - 150, y: 10 };
    expect(restoreBounds(saved, [display], display).options.x).toBe(1770);
  });
  it('drops position when only a sliver is visible', () => {
    expect('x' in restoreBounds({ ...good, x: 1920 - 50 }, [display], display).options).toBe(false);
  });
  it('drops position when the title bar is below the work area', () => {
    expect('y' in restoreBounds({ ...good, y: 1080 }, [display], display).options).toBe(false);
  });
  it('handles multiple displays with negative coordinates', () => {
    const left: Rect = { x: -1280, y: -100, width: 1280, height: 1024 };
    const saved = { ...good, x: -1200, y: -50 };
    expect(restoreBounds(saved, [left, display], display).options).toMatchObject({ x: -1200, y: -50 });
    expect('x' in restoreBounds(saved, [display], display).options).toBe(false);
  });
  it('passes the maximized flag through', () => {
    expect(restoreBounds({ ...good, maximized: true }, [display], display).maximized).toBe(true);
    expect(restoreBounds({ ...good, maximized: true, x: 9000 }, [display], display).maximized).toBe(true);
  });
});

class FakeWindow extends EventEmitter {
  destroyed = false; minimized = false; maximized = false; fullscreen = false;
  bounds: Rect = { x: 10, y: 20, width: 900, height: 650 };
  normal: Rect = { x: 50, y: 60, width: 1100, height: 800 };
  isDestroyed() { return this.destroyed; }
  isMinimized() { return this.minimized; }
  isMaximized() { return this.maximized; }
  isFullScreen() { return this.fullscreen; }
  getBounds() { return this.bounds; }
  getNormalBounds() { return this.normal; }
}

describe('trackWindowState', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('debounces resize/move events', () => {
    const w = new FakeWindow(), save = vi.fn();
    trackWindowState(w, save);
    w.emit('resize'); vi.advanceTimersByTime(300); w.emit('move'); vi.advanceTimersByTime(300);
    expect(save).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    expect(save).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledWith({ x: 10, y: 20, width: 900, height: 650, maximized: false });
  });
  it('saves immediately on close and cancels the pending timer', () => {
    const w = new FakeWindow(), save = vi.fn();
    trackWindowState(w, save);
    w.emit('move'); w.emit('close');
    expect(save).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1000);
    expect(save).toHaveBeenCalledTimes(1);
  });
  it('stores normal bounds when maximized or fullscreen', () => {
    const w = new FakeWindow(), save = vi.fn();
    trackWindowState(w, save);
    w.maximized = true; w.emit('maximize'); vi.advanceTimersByTime(400);
    expect(save).toHaveBeenLastCalledWith({ x: 50, y: 60, width: 1100, height: 800, maximized: true });
    w.maximized = false; w.fullscreen = true; w.emit('resize'); vi.advanceTimersByTime(400);
    expect(save).toHaveBeenLastCalledWith({ x: 50, y: 60, width: 1100, height: 800, maximized: true });
    w.fullscreen = false; w.emit('unmaximize'); vi.advanceTimersByTime(400);
    expect(save).toHaveBeenLastCalledWith({ x: 10, y: 20, width: 900, height: 650, maximized: false });
  });
  it('ignores events while minimized', () => {
    const w = new FakeWindow(), save = vi.fn();
    trackWindowState(w, save);
    w.minimized = true; w.emit('resize'); w.emit('close'); vi.advanceTimersByTime(1000);
    expect(save).not.toHaveBeenCalled();
  });
  it('swallows save errors', () => {
    const w = new FakeWindow(), save = vi.fn(() => { throw new Error('closed'); });
    const t = trackWindowState(w, save);
    w.emit('move');
    expect(() => vi.advanceTimersByTime(400)).not.toThrow();
    expect(() => w.emit('close')).not.toThrow();
    expect(() => t.dispose()).not.toThrow();
    expect(save).toHaveBeenCalled();
  });
  it('dispose flushes and removes listeners', () => {
    const w = new FakeWindow(), save = vi.fn();
    const t = trackWindowState(w, save);
    w.emit('resize'); t.dispose();
    expect(save).toHaveBeenCalledTimes(1);
    expect(w.listenerCount('resize') + w.listenerCount('close') + w.listenerCount('move')).toBe(0);
    w.emit('resize'); vi.advanceTimersByTime(1000);
    expect(save).toHaveBeenCalledTimes(1);
  });
  it('does not save for a destroyed window', () => {
    const w = new FakeWindow(), save = vi.fn();
    const t = trackWindowState(w, save);
    w.destroyed = true; t.flush();
    expect(save).not.toHaveBeenCalled();
  });
});
