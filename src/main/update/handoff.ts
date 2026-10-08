import { spawn } from 'node:child_process';
import { access, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export interface InstallHandoff {
  readonly committed: boolean;
  commit(): Promise<void>;
  cancel(): Promise<void>;
}
// The helper must announce startup and acknowledge commit while Shellfox still
// owns its PID. A spawn event alone says nothing about script startup failures.
export async function startHandoff(file: string, args: string[], control: string, timeoutMs = 60_000): Promise<InstallHandoff> {
  const child = spawn(file, args, { detached: true, stdio: 'ignore', windowsHide: true, shell: false });
  let spawned = false, exited = false, error: Error | null = null, committed = false, cancelled = false;
  child.once('spawn', () => { spawned = true; });
  const ended = new Promise<void>(resolve => {
    child.once('exit', () => { exited = true; resolve(); });
    child.on('error', e => {
      error = e;
      // Spawn errors mean no child exists. A later kill error does NOT prove
      // exit, and a second failed kill must not become an unhandled event.
      if (!spawned) { exited = true; resolve(); }
    });
  });
  async function waitFor(name: string, waitMs = timeoutMs): Promise<void> {
    const deadline = Date.now() + waitMs;
    for (;;) {
      if (exited || cancelled) throw error ?? new Error('Installer helper exited before handoff.');
      try { await access(path.join(control, name)); if (exited || cancelled) throw new Error('Installer helper exited or was cancelled.'); return; }
      catch (e) { if (exited || cancelled) throw e; }
      if (Date.now() >= deadline) throw new Error('Installer helper did not acknowledge handoff.');
      await delay(25);
    }
  }
  let cancellation: Promise<void> | null = null;
  const cancel = (): Promise<void> => {
    if (committed) return Promise.resolve();
    if (cancellation) return cancellation;
    cancelled = true;
    cancellation = (async () => {
    // Before acknowledged commit the helper has not spawned an installer.
    try { await writeFile(path.join(control, 'cancel'), '', { flag: 'wx', mode: 0o600 }); } catch { /* Kill also cancels when storage is unavailable. */ }
    if (!exited) { try { child.kill(); } catch { /* Only confirmed exit permits recovery. */ } }
    await Promise.race([ended, delay(1000, undefined, { ref: false })]);
    if (!exited) {
      try { child.kill('SIGKILL'); } catch { /* Retain the helper if cancellation remains unconfirmed. */ }
      await Promise.race([ended, delay(1000, undefined, { ref: false })]);
    }
    if (!exited) throw new Error('Installer helper cancellation was not confirmed.');
    })().catch(e => { cancellation = null; throw e; });
    return cancellation;
  };
  try { await waitFor('ready'); }
  catch (e) { await cancel(); throw e; }
  return {
    get committed() { return committed; },
    commit: async () => {
      if (committed) return;
      if (cancelled || exited) throw new Error('Installer helper is no longer ready.');
      await writeFile(path.join(control, 'commit'), '', { flag: 'wx', mode: 0o600 });
      await waitFor('committed', Math.min(timeoutMs, 10_000));
      if (cancelled || exited) throw new Error('Installer handoff was cancelled.');
      committed = true;
      child.unref();
    },
    cancel,
  };
}

// Helpers exit if the manager dies without sending commit. A later normal quit
// must never install an update whose terminal shutdown or handoff failed.
export const posixCommitGate = `: > "$control/ready" || exit 1
tries=0
while [ ! -f "$control/commit" ]; do
  [ ! -f "$control/cancel" ] || exit 1
  kill -0 "$pid" 2>/dev/null || exit 1
  tries=$((tries + 1))
  [ "$tries" -le 3000 ] || exit 1
  sleep 0.2
done
[ ! -f "$control/cancel" ] || exit 1
: > "$control/committed" || exit 1
`;
export const powershellCommitGate = `$ready = Join-Path $c.control 'ready'
$commit = Join-Path $c.control 'commit'
$cancel = Join-Path $c.control 'cancel'
New-Item -ItemType File -Path $ready -ErrorAction Stop | Out-Null
$deadline = [DateTime]::UtcNow.AddMinutes(10)
while (-not (Test-Path -LiteralPath $commit)) {
  if ((Test-Path -LiteralPath $cancel) -or -not (Get-Process -Id $c.pid -ErrorAction SilentlyContinue) -or [DateTime]::UtcNow -gt $deadline) { exit 1 }
  Start-Sleep -Milliseconds 200
}
if (Test-Path -LiteralPath $cancel) { exit 1 }
New-Item -ItemType File -Path (Join-Path $c.control 'committed') -ErrorAction Stop | Out-Null
`;
