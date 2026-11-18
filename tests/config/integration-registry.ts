export type IntegrationKind = 'process' | 'acl' | 'pty' | 'pty-ssh' | 'ownership' | 'browser';
const desktop = ['win32', 'linux', 'darwin'] as const;
type Platform = (typeof desktop)[number];
export interface IntegrationCaseFile { path: string; kind: IntegrationKind; platforms: readonly Platform[]; reason: string }
const held = (path: string, kind: IntegrationKind, reason: string, platforms: readonly Platform[] = desktop): IntegrationCaseFile => ({ path, kind, platforms, reason });
/** Selection metadata, never authorization. Every entry remains native execution HOLD. */
export const integrationFiles: readonly IntegrationCaseFile[] = [
  held('tests/unit/cli-tty-driver.test.ts', 'process', 'Real Node driver including invalid-input launches'),
  held('src/host/automation.test.ts', 'process', 'Real Node pipe/supervisor processes including launch errors and lease expiry'),
  held('src/host/automation-lifecycle.test.ts', 'process', 'Node grandchild/unref and numeric PID fallback'),
  held('src/host/cli-binding.test.ts', 'process', 'Real CLI shell binding'),
  held('src/host/ownership.test.ts', 'ownership', 'Real ownership helper including missing-image case; Go3 identity unresolved'),
  held('src/cli/update-activation.test.ts', 'process', 'Real Node proxy'),
  held('src/cli/update-archive.test.ts', 'process', 'Real tar or PowerShell archive'),
  held('src/main/platform/explorer-registry.test.ts', 'process', 'Real PowerShell registry mutation', ['win32']),
  held('src/main/platform/linux-cli.test.ts', 'process', 'Real Linux shell launcher alongside cross-platform source cases'),
  held('src/main/platform/shellfox-cli-scripts.test.ts', 'process', 'Real platform shell/WSL launchers'),
  held('src/main/platform/shellfox-cli.test.ts', 'process', 'Real Windows shims alongside cross-platform modeled cases'),
  held('src/main/platform/shellfox-completion.test.ts', 'process', 'Real completion shells and CLI'),
  held('src/main/platform/wsl-cli.test.ts', 'process', 'Real shell/Python startup files and WSL'),
  held('src/main/platform/wsl-control-launcher.test.ts', 'process', 'Real WSL and host endpoint', ['win32']),
  held('src/main/terminal/environment-nested.test.ts', 'process', 'Real nested shells/WSL'),
  held('src/main/update/handoff-windows.test.ts', 'process', 'Real handoff children and fault actions', ['win32']),
  held('src/main/update/handoff.test.ts', 'process', 'Indirect startHandoff(process.execPath, helper.cjs), fault/timeout'),
  held('src/main/update/launchers.test.ts', 'process', 'Real updater/CLI shells'),
  held('src/main/update/shellfox-update.test.ts', 'process', 'Real Unix updater shell', ['linux', 'darwin']),
  held('src/main/update/shellfox-update-windows.test.ts', 'process', 'Real updater and taskkill fallback', ['win32']),
  held('src/main/ssh/client.e2e.test.ts', 'pty-ssh', 'Build/copy/real PTY/SSH children; WSL remote routing HOLD'),
  held('src/renderer/files/FileCodeEditor.browser.test.ts', 'browser', 'Real Chromium launch via Playwright'),
  held('src/renderer/terminal-links.browser.test.ts', 'browser', 'Real Chromium launch via Playwright'),
  held('src/shared/image-editor.browser.test.ts', 'browser', 'Real Chromium launch via Playwright'),
  held('mobile/src/file-preview.browser.test.ts', 'browser', 'Real Chromium launch via Playwright'),
  held('mobile/src/file-image-preview.browser.test.ts', 'browser', 'Real Chromium launch via Playwright'),
  held('mobile/src/file-source-preview.browser.test.ts', 'browser', 'Real Chromium launch via Playwright'),
  held('src/host/rpc-ownership.os.test.ts', 'ownership', 'Real Go ownership and lifecycle'),
  held('src/cli/service.os.test.ts', 'process', 'Real Node child for packaged installer dispatch, using synthetic installer script'),
  held('src/cli/automation.os.test.ts', 'process', 'CLI over authenticated RPC launches real Node processes'),
  held('src/cli/input-auth-key.os.test.ts', 'acl', 'Synthetic fixture readAuthKey: real PowerShell on Windows, mode path on Unix'),
  held('src/main/directory-drive.os.test.ts', 'process', 'Real PowerShell drive-type verification of a local fixture', ['win32']),
  held('src/main/control-server-acl.os.test.ts', 'acl', 'Real Windows endpoint protection and inherited replacement rejection', ['win32']),
  held('src/main/remote/upload-permissions.os.test.ts', 'acl', 'Real icacls/whoami/PowerShell', ['win32']),
  held('src/main/terminal/backend-adoption.os.test.ts', 'process', 'Real Node pipe child'),
  held('src/main/terminal/cwd-hook.os.test.ts', 'process', 'Real PowerShell cwd', ['win32']),
  held('src/main/terminal/linux-scope.os.test.ts', 'process', 'Real systemd/sleep/OOM probes', ['linux']),
  held('src/main/terminal/unix-close.os.test.ts', 'process', 'Real kernel/tree fault fixture', ['linux', 'win32']),
  held('src/main/terminal/supervisor-native.os.test.ts', 'pty', 'Node C model and Darwin native build/PTY', ['linux', 'darwin']),
  held('src/main/update/platform-installer.os.test.ts', 'process', 'Actual PowerShell parser', ['win32']),
];
export const integrationPaths = integrationFiles.map(entry => entry.path);
export const unitExcludes = ['**/*.e2e.test.{ts,tsx}', '**/*.os.test.{ts,tsx}', ...integrationPaths];
