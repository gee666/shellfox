/** Parse Windows WSL shares without changing the stored host path. */
export function parseWslUnc(cwd: string): { distro: string; guestPath: string } | null {
  const match = /^\\\\(?:wsl\.localhost|wsl\$)\\([^\\/]+)(?:\\(.*))?$/i.exec(cwd);
  return match ? { distro: match[1]!, guestPath: '/' + (match[2] ?? '').replace(/\\/g, '/') } : null;
}
