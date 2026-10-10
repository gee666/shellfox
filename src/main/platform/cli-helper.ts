import path from 'node:path';
export interface CliHelperOptions { executable:string; appPath?:string; cliHelper?:string; userData?:string; prefixArgs?:string[] }
export const quoteSh=(s:string)=>"'"+s.replaceAll("'","'\\''")+"'";
export function helperPath(o:CliHelperOptions):string {
 const p=/^[A-Za-z]:|\\/.test(o.executable)?path.win32:path;
 return o.cliHelper??(o.appPath?p.join(p.dirname(o.appPath),'../cli/main/cli.cjs'):p.join(p.dirname(o.executable),'resources/cli/main/cli.cjs'));
}
export function helperData(o:CliHelperOptions):string|undefined {
 const index=o.prefixArgs?.indexOf('--test-user-data')??-1;
 return o.userData??(index>=0?o.prefixArgs?.[index+1]:undefined);
}
export function posixHelper(o:CliHelperOptions,windows:boolean):string {
 const p=windows?path.win32:path,helper=helperPath(o),runtime=windows?p.join(p.dirname(helper),'../cli-runtime/node.exe'):o.executable;
 const data=helperData(o)??(windows?path.win32.join(process.env.APPDATA??'','Shellfox'):undefined);
 if(!windows)return `export ELECTRON_RUN_AS_NODE=1\nexec ${quoteSh(runtime)} ${quoteSh(helper)} ${data?'--user-data '+quoteSh(data)+' ':''}"$@"\n`;
 return `helper=${quoteSh(helper)}
runtime=${quoteSh(runtime)}
user_data=${quoteSh(data??'')}
if [ -n "\${WSL_DISTRO_NAME:-}" ]; then
  runtime=$(wslpath -u "$runtime") || exit 1
  # Windows Node receives a WSL pseudoconsole; its argv uses Windows paths.
  if [ "\${NO_COLOR+x}" = x ]; then export WSLENV="\${WSLENV:+$WSLENV:}NO_COLOR"; fi
elif command -v cygpath >/dev/null 2>&1; then
  helper=$(cygpath -w "$helper") || exit 1
  runtime=$(cygpath -u "$runtime") || exit 1
fi
unset ELECTRON_RUN_AS_NODE
export MSYS_ARG_CONV_EXCL='*'
if [ -t 0 ] && command -v cygpath >/dev/null 2>&1 && ! "$runtime" -e 'process.exit(process.stdin.isTTY?0:1)' >/dev/null; then
  if ! command -v winpty >/dev/null 2>&1; then printf '%s\\n' '  this terminal requires winpty' >&2; exit 1; fi
  exec winpty "$runtime" "$helper" --user-data "$user_data" "$@"
fi
exec "$runtime" "$helper" ${data?'--user-data "$user_data" ':''}"$@"
`;
}
