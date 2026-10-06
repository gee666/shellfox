import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RELEASES_URL, UPDATE_COMMAND, UpdateChecker, fetchLatestRelease, isNewer, parseVersion, type FetchLike } from './update-check';
import { requestSchemas, responseSchemas } from '../shared/schemas';

describe('isNewer', () => {
  it.each([
    ['0.2.0', '0.1.1', true], ['0.1.2', '0.1.1', true], ['1.0.0', '0.9.9', true], ['0.10.0', '0.9.0', true],
    ['v0.2.0', '0.1.0', true], ['0.2.0', 'v0.1.0', true], ['0.1.1.1', '0.1.1', true], ['0.1.1', '0.1.1.0', false],
    ['0.1.1', '0.1.1', false], ['0.1.0', '0.1.1', false], ['0.9.9', '1.0.0', false], ['1', '1.0.0', false], ['2', '1.9.9', true],
    ['0.2.0-beta.1', '0.1.1', true], ['0.2.0+build5', '0.2.0', false], ['0.1.1-1', '0.1.1', false], ['0.01.0', '0.1.0', false],
    ['', '0.1.1', false], ['garbage', '0.1.1', false], ['0.1.1', '', true], ['0.1.x', '0.1.0', false],
    ['99999999999999999999.0.0', '1.0.0', true],
  ])('isNewer(%j, %j) is %s', (a, b, expected) => { expect(isNewer(a, b)).toBe(expected); });
});
describe('parseVersion', () => {
  it.each([['v0.2.0', '0.2.0'], ['0.2.0', '0.2.0'], ['v1.2.3-rc.1', '1.2.3-rc.1'], [' v1.0 ', '1.0']])('accepts %j', (tag, version) => expect(parseVersion(tag)).toBe(version));
  it.each([[null], [undefined], [5], [''], ['latest'], ['v'], ['1.2.3; rm -rf'], ['9'.repeat(70)]])('rejects %j', tag => expect(parseVersion(tag)).toBeNull());
});
const response = (status: number, body: unknown) => ({ ok: status >= 200 && status < 300, status, json: async () => { if (body === Symbol.for('bad-json')) throw new SyntaxError('bad'); return body; } });
describe('fetchLatestRelease', () => {
  afterEach(() => vi.useRealTimers());
  it('returns the tag without its leading v and sends GitHub headers', async () => {
    const fetchImpl = vi.fn<FetchLike>(async () => response(200, { tag_name: 'v0.3.0', assets: [] }));
    expect(await fetchLatestRelease(fetchImpl, 'https://example.invalid/latest')).toBe('0.3.0');
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe('https://example.invalid/latest');
    expect(init.headers).toMatchObject({ Accept: 'application/vnd.github+json' }); expect(init.signal).toBeInstanceOf(AbortSignal);
  });
  it('returns null for 404 and other HTTP failures', async () => {
    expect(await fetchLatestRelease(async () => response(404, { message: 'Not Found' }))).toBeNull();
    expect(await fetchLatestRelease(async () => response(403, { message: 'rate limit' }))).toBeNull();
    expect(await fetchLatestRelease(async () => response(500, {}))).toBeNull();
  });
  it.each([[null], ['text'], [{}], [{ tag_name: 7 }], [{ tag_name: 'nightly' }], [Symbol.for('bad-json')]])('returns null for malformed body %j', async body => {
    expect(await fetchLatestRelease(async () => response(200, body))).toBeNull();
  });
  it('returns null when the network request throws', async () => {
    expect(await fetchLatestRelease(async () => { throw new TypeError('offline'); })).toBeNull();
  });
  it('gives up after the timeout and aborts the request, even if fetch ignores the signal', async () => {
    vi.useFakeTimers();
    let signal: AbortSignal | undefined;
    const pending = fetchLatestRelease((_url, init) => { signal = init.signal; return new Promise<never>(() => {}); });
    await vi.advanceTimersByTimeAsync(9_999); expect(signal?.aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(2); expect(await pending).toBeNull(); expect(signal?.aborted).toBe(true);
  });
});
describe('UpdateChecker', () => {
  it('reports an available update with the command and releases page', async () => {
    const checker = new UpdateChecker({ current: '0.1.1', check: async () => '0.2.0' });
    expect(await checker.status()).toEqual({ current: '0.1.1', latest: '0.2.0', available: true, command: UPDATE_COMMAND, url: RELEASES_URL });
  });
  it.each([['same', '0.1.1'], ['older', '0.1.0'], ['none', null], ['invalid', 'oops']])('is not available for %s release', async (_name, latest) => {
    expect((await new UpdateChecker({ current: '0.1.1', check: async () => latest }).status()).available).toBe(false);
  });
  it('treats a throwing check as no update', async () => {
    expect(await new UpdateChecker({ current: '0.1.1', check: async () => { throw new Error('boom'); } }).status()).toMatchObject({ latest: null, available: false });
  });
  it('checks once per start and shares the result', async () => {
    const check = vi.fn(async () => '0.2.0'); const checker = new UpdateChecker({ current: '0.1.1', check });
    const [a, b] = await Promise.all([checker.status(), checker.status()]); await checker.status();
    expect(a).toBe(b); expect(check).toHaveBeenCalledTimes(1);
  });
});
describe('getUpdateStatus schemas', () => {
  const status = { current: '0.1.1', latest: '0.2.0', available: true, command: 'shellfox update', url: RELEASES_URL };
  it('accepts only an empty request', () => {
    expect(requestSchemas.getUpdateStatus.safeParse({}).success).toBe(true);
    expect(requestSchemas.getUpdateStatus.safeParse({ extra: 1 }).success).toBe(false);
  });
  it('validates the response strictly', () => {
    expect(responseSchemas.getUpdateStatus.safeParse({ ok: true, value: status }).success).toBe(true);
    expect(responseSchemas.getUpdateStatus.safeParse({ ok: true, value: { ...status, latest: null, available: false } }).success).toBe(true);
    expect(responseSchemas.getUpdateStatus.safeParse({ ok: true, value: { ...status, command: 'rm -rf /' } }).success).toBe(false);
    expect(responseSchemas.getUpdateStatus.safeParse({ ok: true, value: { ...status, extra: 1 } }).success).toBe(false);
  });
});
