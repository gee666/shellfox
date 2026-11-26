// A fresh simulator per CI run: a reused device can carry a half-finished boot
// or state from an earlier job on the same runner image.
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { run } from '../../../scripts/rust-package-common.mjs';

const simctl = (args, timeout = 60_000) => run('xcrun', ['simctl', ...args], { timeout });

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

/** Creates a device, boots it and waits until SpringBoard is ready. */
export async function freshSimulator(family) {
  const { devices } = JSON.parse(simctl(['list', '-j', 'devices', 'available']));
  const { runtime, deviceType, name } = template(devices, family);
  const udid = simctl(['create', `Shellfox CI ${name}`, deviceType, runtime]).trim();
  for (let attempt = 1; ; attempt += 1) {
    try {
      // bootstatus -b boots the device if needed and returns once it is usable.
      // Progress goes to stderr: the CLI's stdout is only the UDID.
      run('xcrun', ['simctl', 'bootstatus', udid, '-b'], { timeout: 300_000, stdio: ['ignore', 2, 2] });
      return { udid, name };
    } catch (error) {
      if (attempt === 3) {
        removeSimulator(udid);
        throw error;
      }
      console.warn(`Simulator boot attempt ${attempt} failed; erasing and retrying: ${error.message}`);
      try { simctl(['shutdown', udid]); } catch { /* It may never have booted. */ }
      // A boot that hangs usually means CoreSimulatorService itself is stuck on
      // this runner; launchd restarts it on the next simctl call.
      try { run('/usr/bin/killall', ['-9', 'com.apple.CoreSimulator.CoreSimulatorService']); } catch { /* Not running. */ }
      try { simctl(['erase', udid], 120_000); } catch (erase) { console.warn(`Could not erase ${udid}: ${erase.message}`); }
      await delay(attempt * 10_000);
    }
  }
}

export function removeSimulator(udid) {
  try { simctl(['shutdown', udid]); } catch { /* Already shut down. */ }
  try { simctl(['delete', udid]); } catch (error) { console.warn(`Could not delete simulator ${udid}: ${error.message}`); }
}

// CLI for the workflow: `create <family>` prints the UDID, `delete <udid>` removes it.
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const [command, value] = process.argv.slice(2);
  if (command === 'create' && value) {
    const { udid } = await freshSimulator(value);
    process.stdout.write(`${udid}\n`);
  } else if (command === 'delete' && value) removeSimulator(value);
  else throw new Error('Usage: simulator.mjs create <family> | delete <udid>');
}
