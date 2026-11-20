import assert from 'node:assert/strict';
import test from 'node:test';
import { listenerStopped, requestListenerStop } from './confirmed-network-outage.mjs';

const retained = { enabled: false, status: 'error', cleanupRetained: true, error: { code: 'NATIVE_UNAVAILABLE' } };
const stopped = { enabled: false, status: 'disabled', cleanupRetained: false, error: null };

test('a retained disable receipt does not confirm listener shutdown', async () => {
  const commands = [];
  await requestListenerStop((...args) => {
    commands.push(args);
    if (args.at(-1) === 'off') throw new Error('Local network operator is not confirmed');
    return retained;
  });
  assert.deepEqual(commands, [['host', 'network', 'off'], ['host', 'network', 'status']]);
  assert.equal(listenerStopped(retained), false);
  assert.equal(listenerStopped(stopped), true);
});

test('an unrelated stop failure still fails the fixture', async () => {
  const failure = new Error('stop failed');
  await assert.rejects(requestListenerStop((...args) => {
    if (args.at(-1) === 'off') throw failure;
    return { enabled: true, status: 'online', cleanupRetained: false, error: null };
  }), error => error === failure);
});
