import { launchConfig } from './shellfox-cli-launch';
const quote = (text: string) => "'" + text.replaceAll("'", "''") + "'";

/** PS 5.1's native argv conversion drops empty values and mangles embedded quotes.
 * Pass user arguments as base64 JSON instead, including fixed helper arguments. */
export function powershellShim(runtime: string, helper: string, userData: string | undefined,
  executable: string, appArgs: string[], environment: Record<string, string>, hostEnvironment: Record<string, string> = {}): string {
  const config = launchConfig(executable, appArgs, environment);
  const fixed = userData ? `${quote('--shellfox-default-user-data')}, ${quote(userData)}` : '';
  return `# Shellfox/cli-v1
$ErrorActionPreference='Stop'
$names=@('ELECTRON_RUN_AS_NODE','SHELLFOX_CLI_DISTRO','SHELLFOX_CLI_WSL_CWD','SHELLFOX_CLI_GIT_BASH','SHELLFOX_CLI_GIT_CWD','SHELLFOX_CLI_LAUNCH','SHELLFOX_CLI_UPDATE','SHELLFOX_RESOURCES_PATH','SHELLFOX_PROJECT_ROOT','SHELLFOX_RUNTIME_PATH')
$saved=@{}
foreach ($name in $names) { $saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process') }
$code=1
try {
  foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name,$null,'Process') }
  $env:SHELLFOX_CLI_LAUNCH=${quote(config)}
  $env:SHELLFOX_CLI_UPDATE=Join-Path $PSScriptRoot 'shellfox-update.ps1'
${Object.entries(hostEnvironment).map(([name, value]) => `  [Environment]::SetEnvironmentVariable(${quote(name)},${quote(value)},'Process')`).join('\n')}
  [string[]]$forward=@(${fixed}) + @($args | ForEach-Object { [string]$_ })
  $json=ConvertTo-Json -Compress -InputObject @($forward)
  $payload=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
  & ${quote(runtime)} ${quote(helper)} '--shellfox-argv-json' $payload
  $code=$LASTEXITCODE
} finally {
  foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name,$saved[$name],'Process') }
}
exit $code
`;
}
