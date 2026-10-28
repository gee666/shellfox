import { expect, it, vi } from 'vitest';
import { ScreenMirror } from './screen-mirror';

function expectReleased(mirror: ScreenMirror) {
  for (const key of ['term', 'serializer', 'parseNow']) expect(Reflect.get(mirror, key) === null, key).toBe(true);
}

it('freezes the final VT state and title without retaining a terminal or repeating device replies', () => {
  const reply = vi.fn();
  const mirror = new ScreenMirror(80, 24, undefined, reply);
  mirror.write(1, '\x1b]2;finished shell\x07\x1b[?1049h\x1b[?25l\x1b[5;8Hfinal screen\x1b[6n');
  const expected = mirror.snapshot(262144);
  expect(expected).not.toBeNull();
  const replies = reply.mock.calls.length;
  mirror.finish(262144);
  expectReleased(mirror);
  expect(mirror.snapshot(262144)).toEqual(expected);
  expect(mirror.title).toBe('finished shell');
  mirror.write(2, 'late output\x1b[6n');
  mirror.resize(120, 40);
  mirror.finish(262144);
  expect(mirror.sequence).toBe(1);
  expect(mirror.snapshot(262144)).toEqual(expected);
  expect(reply).toHaveBeenCalledTimes(replies);
  const snapshot = mirror.snapshot(262144)!;
  snapshot.data = 'caller mutation';
  expect(mirror.snapshot(262144)).toEqual(expected);
  mirror.dispose();
  expect(mirror.snapshot(262144)).toBeNull();
});

it('bounds frozen history by bytes and releases the parser even when no snapshot fits', () => {
  const mirror = new ScreenMirror(80, 24);
  mirror.write(1, Array.from({ length: 3100 }, (_, i) => `${i}: ${'界'.repeat(30)}`).join('\r\n'));
  mirror.finish(40 * 1024);
  expectReleased(mirror);
  const snapshot = mirror.snapshot(40 * 1024)!;
  expect(Buffer.byteLength(snapshot.data)).toBeLessThanOrEqual(40 * 1024);
  expect(snapshot.data).toContain('3099:');
  expect(mirror.snapshot(10)).toBeNull();
  mirror.dispose();
  const tooSmall = new ScreenMirror(80, 24);
  tooSmall.write(1, 'screen');
  tooSmall.finish(1);
  expectReleased(tooSmall);
  expect(tooSmall.snapshot(262144)).toBeNull();
  tooSmall.dispose();
});

it('drains public-write fallback output and resizes before releasing the parser', async () => {
  const mirror = new ScreenMirror(80, 24);
  // Exercise the fallback used if a future xterm no longer exposes synchronous parse.
  Reflect.set(mirror, 'parseNow', null);
  mirror.write(1, 'before resize');
  mirror.resize(100, 30);
  mirror.write(2, '\x1b[30;1Hlast output');
  mirror.finish(262144);
  mirror.write(3, 'must not be admitted');
  for (let i = 0; i < 100 && Reflect.get(mirror, 'term') !== null; i++) await new Promise(resolve => setTimeout(resolve, 1));
  expectReleased(mirror);
  expect(mirror.snapshot(262144)).toMatchObject({ sequence: 2, cols: 100, rows: 30, data: expect.stringContaining('last output') });
  mirror.dispose();
});
