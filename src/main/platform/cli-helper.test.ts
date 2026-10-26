import { afterEach, expect, it, vi } from 'vitest';
afterEach(() => vi.unstubAllEnvs());
import { helperData, helperHostEnvironment, helperPath, posixHelper } from './cli-helper';
it('uses the exact helper and isolated user data paths', () => {
 const options = { executable: 'C:\\Program Files\\Shellfox\\shellfox.exe', appPath: 'P:\\build\\main\\index.cjs', prefixArgs: ['--test-user-data', 'P:\\test data'] };
 expect(helperPath(options)).toBe('P:\\build\\cli\\main\\cli.cjs');
 expect(helperData(options)).toBe('P:\\test data');
 expect(helperData({ ...options, userData: 'P:\\override' })).toBe('P:\\override');
 expect(helperPath({ executable: '/opt/shellfox/shellfox' })).toBe('/opt/shellfox/resources/cli/main/cli.cjs');
});
it('binds packaged resources and development project roots independently of caller cwd', () => {
 expect(helperHostEnvironment({ executable: 'C:\\app\\Shellfox.exe' })).toEqual({ SHELLFOX_RESOURCES_PATH: 'C:\\app\\resources' });
 expect(helperHostEnvironment({ executable: '/Applications/Shellfox.app/Contents/MacOS/Shellfox', cliHelper: '/Applications/Shellfox.app/Contents/Resources/cli/main/cli.cjs' })).toEqual({ SHELLFOX_RESOURCES_PATH: '/Applications/Shellfox.app/Contents/Resources' });
 expect(helperHostEnvironment({ executable: '/node', appPath: '/repo/tmp/build/main/index.cjs', cliHelper: '/repo/tmp/build/cli/main/cli.cjs' })).toEqual({ SHELLFOX_PROJECT_ROOT: '/repo' });
 expect(helperHostEnvironment({ executable: '/node', cliHelper: '/custom/cli.cjs' })).toEqual({});
});
it('forwards the live marker to Windows Node through WSLENV without converting it to a path', () => {
 const script = posixHelper({ executable: 'C:\\Shellfox\\shellfox.exe', userData: 'C:\\data' }, true);
 expect(script).toContain('if [ -n "${SHELLFOX_TERMINAL_MARKER:-}" ]; then shellfox_wsl_env SHELLFOX_TERMINAL_MARKER; fi');
 expect(script).toContain('export WSLENV="${shellfox_env_keep:+$shellfox_env_keep:}$1"');
 expect(script).toContain('shellfox_env_name=$(printf');
 expect(script).not.toContain('SHELLFOX_TERMINAL_MARKER/p');
 expect(script).toContain('NO_COLOR'); expect(script).toContain('unset ELECTRON_RUN_AS_NODE');
 expect(script).toContain('"$@"'); expect(script).toContain('wslpath -u "$runtime"');
});
it('leaves Windows default data resolution to Windows Node when APPDATA is absent', () => {
 vi.stubEnv('APPDATA', undefined);
 const script = posixHelper({ executable: 'C:\\Shellfox\\shellfox.exe' }, true);
 expect(script).not.toContain('--user-data');
 expect(script).not.toContain("user_data='Shellfox'");
 expect(script).toContain('exec "$runtime" "$helper" "$@"');
});
it('scopes the Electron Node mode to the POSIX helper invocation and quotes data', () => {
 const script = posixHelper({ executable: '/opt/app name', userData: "/data/it's here", cliHelper: '/helper file' }, false);
 expect(script).toBe("unset SHELLFOX_RESOURCES_PATH SHELLFOX_PROJECT_ROOT SHELLFOX_RUNTIME_PATH\nexport ELECTRON_RUN_AS_NODE=1\nexec '/opt/app name' '/helper file' --user-data '/data/it'\\''s here' \"$@\"\n");
});
