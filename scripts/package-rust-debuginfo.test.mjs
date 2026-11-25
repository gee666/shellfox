import assert from 'node:assert/strict';
import test from 'node:test';
import { debugInfoEnv } from './package-rust-debuginfo.mjs';

test('only macOS needs a build setting to split its symbols', () => {
  assert.deepEqual(debugInfoEnv('linux'), {});
  assert.deepEqual(debugInfoEnv('darwin'), { CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO: 'packed' });
  assert.deepEqual(debugInfoEnv('win32'), {});
});
