import assert from 'node:assert/strict';
import test from 'node:test';
import { assertDebugGrowth, debugInfoEnv, LINUX_DEBUG_LIMIT } from './package-rust-debuginfo.mjs';

test('each platform keeps its line tables its own way', () => {
  assert.deepEqual(debugInfoEnv('linux', {}), { RUSTFLAGS: '-C link-arg=-Wl,--compress-debug-sections=zlib' });
  assert.equal(debugInfoEnv('linux', { RUSTFLAGS: '-C x' }).RUSTFLAGS, '-C x -C link-arg=-Wl,--compress-debug-sections=zlib');
  assert.deepEqual(debugInfoEnv('darwin', {}), { CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO: 'packed' });
  assert.deepEqual(debugInfoEnv('win32', {}), {});
});

test('the Linux binary may grow by at most 15 MB', () => {
  assert.equal(assertDebugGrowth(LINUX_DEBUG_LIMIT + 10, 10, 'shellfox'), LINUX_DEBUG_LIMIT);
  assert.throws(() => assertDebugGrowth(LINUX_DEBUG_LIMIT + 11, 10, 'shellfox'), /over the 15 MB limit/);
});
