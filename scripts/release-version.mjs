// One release version, mapped to each package format. A version is X.Y.Z or a
// test build X.Y.Z-alpha.N, -beta.N or -rc.N (N = 1..32). Every mapping sorts
// test builds below their final release and above the previous one.
const PATTERN = /^(0|[1-9]\d{0,2})\.(0|[1-9]\d{0,2})\.(0|[1-9]\d{0,2})(?:-(alpha|beta|rc)\.([1-9]\d?))?$/;
const CHANNELS = { alpha: 0, beta: 32, rc: 64 };
const STABLE_BUILD = 99;

export function releaseVersion(version) {
  const match = typeof version === 'string' ? PATTERN.exec(version) : null;
  const number = match?.[5] ? Number(match[5]) : null;
  if (!match || (number !== null && number > 32)) throw new Error(`Unsupported release version: ${version}`);
  const [major, minor, patch] = match.slice(1, 4).map(Number);
  const core = `${major}.${minor}.${patch}`;
  const pre = match[4] ? { channel: match[4], number } : null;
  // 99 for a final release; channel offset + N for test builds.
  const build = pre ? CHANNELS[pre.channel] + pre.number : STABLE_BUILD;
  // Android caps versionCode at 2100000000.
  const code = (major * 1_000_000 + minor * 1000 + patch) * 100 + build;
  if (!code || code > 2_100_000_000) throw new Error(`Release version is out of range: ${version}`);
  return {
    version,
    core,
    prerelease: pre !== null,
    // Debian: "~" sorts before the empty string, "-" would mean a package revision.
    deb: pre ? `${core}~${pre.channel}.${pre.number}` : core,
    // Squirrel's NuGet 2 accepts only SemVer 1 labels: no dots, compared as text.
    nuget: pre ? `${core}-${pre.channel}${String(pre.number).padStart(2, '0')}` : core,
    // Android versionCode, Apple bundle version and the Windows fourth version field.
    build,
    code,
  };
}
