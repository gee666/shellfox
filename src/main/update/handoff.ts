import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { access, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export interface InstallHandoff {
  readonly committed: boolean;
  commit(): Promise<void>;
  cancel(): Promise<void>;
}
// Start-Process uses a separate console on Windows. DETACHED_PROCESS prevents
// PS5.1 from running scripts, while a non-detached libuv child dies with Node.
// Keep the bootstrap alive until release, holding the actual helper's process
// handle so cancellation cannot confuse bootstrap exit or PID reuse with exit.
function windowsBootstrap(file: string, args: string[], control: string): string[] {
  // Start-Process joins ArgumentList into a native command line. Quote each
  // argument using Windows argv rules, not PowerShell or cmd.exe syntax.
  const quote = (arg: string) => '"' + arg.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/g, '$1$1') + '"';
  const config = Buffer.from(JSON.stringify({ file, arguments: args.map(quote).join(' '), control, pid: process.pid }), 'utf8').toString('base64');
  const script = `$ErrorActionPreference = 'Stop'
$c = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${config}')) | ConvertFrom-Json
$cancel = Join-Path $c.control 'cancel'
$release = Join-Path $c.control 'release'
$released = Join-Path $c.control 'released'
$detach = Join-Path $c.control 'detach'
$exited = Join-Path $c.control 'helper-exited'
$p = $null
try {
  if (-not (Test-Path -LiteralPath $cancel)) {
    $p = Start-Process -FilePath $c.file -ArgumentList $c.arguments -WindowStyle Hidden -PassThru
    $null = $p.Handle
    [IO.File]::WriteAllText((Join-Path $c.control 'helper-process.json'), (ConvertTo-Json -Compress @{ pid = $p.Id; bootstrapPid = $PID }))
  }
  $deadline = [DateTime]::UtcNow.AddMinutes(10)
  while ($p -and -not $p.HasExited) {
    if ((Test-Path -LiteralPath $cancel) -or [DateTime]::UtcNow -gt $deadline) {
      try { $p.Kill(); $p.WaitForExit(1000) | Out-Null } catch {}
    } elseif ((Test-Path -LiteralPath $detach) -and (Test-Path -LiteralPath $released)) {
      # Only the manager can detach us, after the live-helper release handshake.
      exit 0
    } elseif (-not (Get-Process -Id $c.pid -ErrorAction SilentlyContinue)) {
      try { $p.Kill(); $p.WaitForExit(1000) | Out-Null } catch {}
    } elseif ((Test-Path -LiteralPath $release) -and -not (Test-Path -LiteralPath $released)) {
      [IO.File]::WriteAllText($released, '')
    }
    Start-Sleep -Milliseconds 50
  }
} catch {
  try { [IO.File]::WriteAllText((Join-Path $c.control 'bootstrap-error'), ($_ | Out-String)) } catch {}
  # Never abandon an unconfirmed live helper, even if recording its PID failed.
  while ($p -and -not $p.HasExited) {
    try { $p.Kill(); $p.WaitForExit(1000) | Out-Null } catch {}
    Start-Sleep -Milliseconds 50
  }
}
# This marker certifies actual helper exit, or that no helper was created.
[IO.File]::WriteAllText($exited, '')
`;
  return ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')];
}

async function hasFile(file: string): Promise<boolean> {
  try { await access(file); return true; }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false; throw error; }
}

// The helper must announce startup and acknowledge commit while Shellfox still
// owns its PID. A spawn event alone says nothing about script startup failures.
export async function startHandoff(file: string, args: string[], control: string, timeoutMs = 60_000): Promise<InstallHandoff> {
  const windows = process.platform === 'win32' && path.win32.basename(file).toLowerCase() === 'powershell.exe';
  const child = spawn(file, windows ? windowsBootstrap(file, args, control) : args, { detached: !windows, stdio: 'ignore', windowsHide: true, shell: false });
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
      if (exited || cancelled || (windows && await hasFile(path.join(control, 'helper-exited')))) throw error ?? new Error('Installer helper exited before handoff.');
      try {
        await access(path.join(control, name));
        if (exited || cancelled || (windows && await hasFile(path.join(control, 'helper-exited')))) throw new Error('Installer helper exited or was cancelled.');
        return;
      }
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
    try { await writeFile(path.join(control, 'cancel'), '', { flag: 'wx', mode: 0o600 }); } catch { /* Direct children can still be killed; Windows requires positive confirmation. */ }
    if (windows) {
      const deadline = Date.now() + 5000;
      // The bootstrap's exit alone does not certify its independent child died.
      while (spawned || !exited) {
        if (await hasFile(path.join(control, 'helper-exited'))) break;
        if (Date.now() >= deadline) throw new Error('Installer helper cancellation was not confirmed.');
        await delay(25);
      }
    }
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
      if (windows) {
        // The bootstrap still owns the actual process handle through this ack.
        await writeFile(path.join(control, 'release'), '', { flag: 'wx', mode: 0o600 });
        await waitFor('released', Math.min(timeoutMs, 10_000));
        if (cancelled || exited) throw new Error('Installer handoff was cancelled.');
        // Detach and the JS committed flag share one synchronous turn. A racing
        // cancel must not detach an independent helper and then resume the app.
        writeFileSync(path.join(control, 'detach'), '', { flag: 'wx', mode: 0o600 });
      }
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
