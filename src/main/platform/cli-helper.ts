import path from 'node:path';
export interface CliHelperOptions { executable:string; appPath?:string; cliHelper?:string; userData?:string; prefixArgs?:string[] }
export const quoteSh=(s:string)=>"'"+s.replaceAll("'","'\\''")+"'";
export function helperPath(o:CliHelperOptions):string {
 const p=/^[A-Za-z]:|\\/.test(o.executable)?path.win32:path.posix;
 return o.cliHelper??(o.appPath?p.join(p.dirname(o.appPath),'../cli/main/cli.cjs'):p.join(p.dirname(o.executable),'resources/cli/main/cli.cjs'));
}
export function helperData(o:CliHelperOptions):string|undefined {
 const index=o.prefixArgs?.indexOf('--test-user-data')??-1;
 return o.userData??(index>=0?o.prefixArgs?.[index+1]:undefined);
}
export function posixHelper(o:CliHelperOptions,windows:boolean):string {
 const p=windows?path.win32:path.posix,helper=helperPath(o),runtime=windows?p.join(p.dirname(helper),'../cli-runtime/node.exe'):o.executable;
 const data=helperData(o)??(windows&&process.env.APPDATA?path.win32.join(process.env.APPDATA,'Shellfox'):undefined);
 if(!windows)return `export ELECTRON_RUN_AS_NODE=1\nexec ${quoteSh(runtime)} ${quoteSh(helper)} ${data?'--user-data '+quoteSh(data)+' ':''}"$@"\n`;
 return `helper=${quoteSh(helper)}
runtime=${quoteSh(runtime)}
user_data=${quoteSh(data??'')}
if [ -n "\${WSL_DISTRO_NAME:-}" ]; then
  runtime=$(wslpath -u "$runtime") || exit 1
  # Windows Node reads the Windows endpoint and connects to Windows loopback.
  # Keep its helper/data argv as Windows paths. Never fall back to guest Node.
  shellfox_wsl_env() {
    shellfox_env_rest=\${WSLENV:-}
    shellfox_env_keep=
    while [ -n "$shellfox_env_rest" ]; do
      shellfox_env_entry=\${shellfox_env_rest%%:*}
      case "$shellfox_env_rest" in *:*) shellfox_env_rest=\${shellfox_env_rest#*:} ;; *) shellfox_env_rest= ;; esac
      shellfox_env_name=$(printf '%s' "\${shellfox_env_entry%%/*}" | tr '[:lower:]' '[:upper:]')
      if [ -n "$shellfox_env_entry" ] && [ "$shellfox_env_name" != "$1" ]; then
        shellfox_env_keep=\${shellfox_env_keep:+$shellfox_env_keep:}$shellfox_env_entry
      fi
    done
    # Replace old /p, /l or /w flags and duplicates on managed names only.
    export WSLENV="\${shellfox_env_keep:+$shellfox_env_keep:}$1"
  }
  if [ "\${NO_COLOR+x}" = x ]; then shellfox_wsl_env NO_COLOR; fi
  if [ -n "\${SHELLFOX_TERMINAL_MARKER:-}" ]; then shellfox_wsl_env SHELLFOX_TERMINAL_MARKER; fi
elif command -v cygpath >/dev/null 2>&1; then
  helper=$(cygpath -w "$helper") || exit 1
  runtime=$(cygpath -u "$runtime") || exit 1
fi
unset ELECTRON_RUN_AS_NODE
export MSYS_ARG_CONV_EXCL='*'
if [ -t 0 ] && command -v cygpath >/dev/null 2>&1 && ! "$runtime" -e 'process.exit(process.stdin.isTTY?0:1)' >/dev/null; then
  if ! command -v winpty >/dev/null 2>&1; then printf '%s\\n' '  this terminal requires winpty' >&2; exit 1; fi
  exec winpty "$runtime" "$helper" ${data?'--user-data "$user_data" ':''}"$@"
fi
exec "$runtime" "$helper" ${data?'--user-data "$user_data" ':''}"$@"
`;
}
