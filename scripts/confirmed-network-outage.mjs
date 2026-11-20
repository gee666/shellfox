import assert from 'node:assert/strict';

export function listenerStopped(state) {
  return state.enabled === false && state.status === 'disabled' && state.cleanupRetained === false && state.error === null;
}

export async function requestListenerStop(cli) {
  try {
    assert.equal((await cli('host', 'network', 'off')).enabled, false);
  } catch (error) {
    // The CLI exits nonzero when the original helper's exit is still pending.
    // Never restore a listener until a later status read confirms cleanup.
    const state = await cli('host', 'network', 'status');
    const stopping = state.enabled === false && state.cleanupRetained === true &&
      state.status === 'error' && state.error?.code === 'NATIVE_UNAVAILABLE';
    if (!stopping && !listenerStopped(state)) throw error;
  }
}
