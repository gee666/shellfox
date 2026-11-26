import assert from 'node:assert/strict';
import test from 'node:test';
import { template } from './simulator.mjs';

const device = (name, type, available = true) => ({ name, deviceTypeIdentifier: type, isAvailable: available });

test('picks the device type from the newest iOS runtime', () => {
  const devices = {
    'com.apple.CoreSimulator.SimRuntime.iOS-18-6': [device('iPad Air', 'ipad-air')],
    'com.apple.CoreSimulator.SimRuntime.iOS-26-1': [device('iPhone 17', 'iphone-17'), device('iPad Pro 13-inch (M5)', 'ipad-pro')],
    'com.apple.CoreSimulator.SimRuntime.iOS-26-0': [device('iPad Pro 11-inch', 'ipad-old')],
    'com.apple.CoreSimulator.SimRuntime.watchOS-26-0': [device('iPad lookalike', 'watch')],
  };
  assert.deepEqual(template(devices, 'iPad'),
    { runtime: 'com.apple.CoreSimulator.SimRuntime.iOS-26-1', deviceType: 'ipad-pro', name: 'iPad Pro 13-inch (M5)' });
  assert.equal(template(devices, 'iPhone').deviceType, 'iphone-17');
});

test('skips unavailable devices and fails when none fits', () => {
  const devices = { 'com.apple.CoreSimulator.SimRuntime.iOS-26-1': [device('iPad Pro', 'ipad-pro', false)] };
  assert.throws(() => template(devices, 'iPad'), /No available iPad simulator/);
});

test('finds a device state across runtimes', async () => {
  const { deviceState } = await import('./simulator.mjs');
  const list = { 'com.apple.CoreSimulator.SimRuntime.iOS-26-1': [{ udid: 'A', state: 'Booted' }, { udid: 'B', state: 'Shutdown' }] };
  assert.equal(deviceState(list, 'B'), 'Shutdown');
  assert.equal(deviceState(list, 'C'), undefined);
});
