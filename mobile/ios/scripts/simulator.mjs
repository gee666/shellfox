// One fresh simulator per CI job, shared by the native contracts and the UI
// fixture. A new device's first boot runs a data migration that can take many
// minutes on a cold runner, so the job starts it early and waits only where a
// test needs it. Erasing would restart that migration, so recovery never erases.
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { run } from '../../../scripts/rust-package-common.mjs';

const simctl = (args, timeout = 60_000) => run('xcrun', ['simctl', ...args], { timeout });
const SERVICE = 'com.apple.CoreSimulator.CoreSimulatorService';

/** The newest available runtime's device type whose name starts with `family`. */
export function template(devices, family) {
  const runtimes = Object.keys(devices).filter(id => /SimRuntime\.iOS-/.test(id))
    .sort((a, b) => version(b) - version(a));
  for (const runtime of runtimes) {
    const device = devices[runtime].find(item => item.isAvailable !== false && item.name.startsWith(family)
      && item.deviceTypeIdentifier);
    if (device) return { runtime, deviceType: device.deviceTypeIdentifier, name: device.name };
  }
  throw new Error(`No available ${family} simulator`);
}

function version(runtime) {
  const [major = 0, minor = 0] = runtime.split('iOS-')[1].split('-').map(Number);
  return major * 1000 + minor;
}

/** The device list, after restarting CoreSimulatorService once if it does not answer. */
export async function devices() {
  for (let attempt = 1; ; attempt += 1) {
    try { return JSON.parse(simctl(['list', '-j', 'devices'])).devices; }
    catch (error) {
      if (attempt === 2) throw new Error(`CoreSimulatorService does not answer: ${error.message}`);
      console.warn(`simctl list failed; restarting CoreSimulatorService: ${error.message}`);
      // launchd starts it again on the next simctl call.
      try { run('/usr/bin/killall', ['-9', SERVICE]); } catch { /* Not running. */ }
      await delay(5_000);
    }
  }
}

export function deviceState(list, udid) {
  return Object.values(list).flat().find(device => device.udid === udid)?.state;
}

/** Creates a device and starts its boot without waiting for it. */
export async function createSimulator(family) {
  const { runtime, deviceType, name } = template(await devices(), family);
  const udid = simctl(['create', `Shellfox CI ${name}`, deviceType, runtime]).trim();
  simctl(['boot', udid], 120_000);
  console.warn(`Started booting ${name} (${udid})`);
  return udid;
}

/** Waits until the device is usable. A first boot may take most of `budgetMs`. */
export async function waitForBoot(udid, budgetMs = 20 * 60_000) {
  const deadline = Date.now() + budgetMs;
  for (let attempt = 1; ; attempt += 1) {
    const state = deviceState(await devices(), udid);
    if (!state) throw new Error(`Simulator ${udid} does not exist`);
    try {
      // -b boots it again if a recovery shut it down. Progress goes to stderr,
      // so the CLI's stdout stays machine-readable.
      run('xcrun', ['simctl', 'bootstatus', udid, '-b'],
        { timeout: Math.max(60_000, deadline - Date.now()), stdio: ['ignore', 2, 2] });
      if (deviceState(await devices(), udid) !== 'Booted') throw new Error(`Simulator ${udid} is not booted`);
      return;
    } catch (error) {
      if (attempt === 2 || Date.now() > deadline - 3 * 60_000) throw error;
      console.warn(`Simulator ${udid} did not finish booting; restarting it: ${error.message}`);
      try { simctl(['shutdown', udid]); } catch { /* It may be half booted. */ }
      try { run('/usr/bin/killall', ['-9', SERVICE]); } catch { /* Not running. */ }
      await delay(10_000);
    }
  }
}

export function removeSimulator(udid) {
  try { simctl(['shutdown', udid]); } catch { /* Already shut down. */ }
  try { simctl(['delete', udid]); } catch (error) { console.warn(`Could not delete simulator ${udid}: ${error.message}`); }
}

// CLI for the workflow. `create <family>` prints the UDID of a device that is
// booting; `wait <udid>` returns once it is usable; `delete <udid>` removes it.
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const [command, value] = process.argv.slice(2);
  if (command === 'create' && value) process.stdout.write(`${await createSimulator(value)}\n`);
  else if (command === 'wait' && value) await waitForBoot(value);
  else if (command === 'delete' && value) removeSimulator(value);
  else throw new Error('Usage: simulator.mjs create <family> | wait <udid> | delete <udid>');
}
