import { describe, expect, it } from 'vitest';
import { isNewerVersion, MAX_APK_BYTES, parseAndroidRelease } from './releases';

export const asset = {
  name: 'shellfox-mobile-0.9.0-android.apk', size: 123,
  browser_download_url: 'https://github.com/gee666/shellfox/releases/download/v0.9.0/shellfox-mobile-0.9.0-android.apk',
  digest: `sha256:${'AB'.repeat(32)}`,
};
export const release = { tag_name: 'v0.9.0', draft: false, prerelease: false, assets: [asset] };

describe('stable APK release validation', () => {
  it('selects only the expected repository asset and normalizes its checksum', () => {
    expect(parseAndroidRelease(release, '0.8.0')).toEqual({
      version: '0.9.0', url: asset.browser_download_url, sha256: 'ab'.repeat(32), size: 123,
    });
  });
  it.each(['0.9.0', '1.0.0', 'garbage', '0.08.0', '0.8.0-beta'])('rejects a non-upgrade from %s', current => {
    expect(parseAndroidRelease(release, current)).toBeNull();
  });
  it.each(['0.9.0', 'v0.9.0-beta.1', 'v00.9.0', 'v0.9.0+build', 'v9007199254740992.0.0'])('rejects invalid stable tag %s', tag_name => {
    expect(parseAndroidRelease({ ...release, tag_name }, '0.8.0')).toBeNull();
  });
  it.each([{ draft: true }, { prerelease: true }, { draft: undefined }, { prerelease: undefined }, { assets: [] }, { assets: [asset, asset] }])('rejects draft, prerelease, incomplete or ambiguous metadata %j', override => {
    expect(parseAndroidRelease({ ...release, ...override }, '0.8.0')).toBeNull();
  });
  it.each([
    'http://github.com/gee666/shellfox/releases/download/v0.9.0/shellfox-mobile-0.9.0-android.apk',
    asset.browser_download_url.replace('gee666', 'attacker'),
    asset.browser_download_url.replace('github.com', 'github.com.attacker.example'),
    asset.browser_download_url.replace('github.com', 'user@github.com'),
    asset.browser_download_url.replace('github.com', 'github.com:443'),
    `${asset.browser_download_url}?download=1`, `${asset.browser_download_url}#fragment`,
    asset.browser_download_url.replace('/v0.9.0/', '/v0.8.0/'),
  ])('rejects untrusted URL %s', browser_download_url => {
    expect(parseAndroidRelease({ ...release, assets: [{ ...asset, browser_download_url }] }, '0.8.0')).toBeNull();
  });
  it.each([{ digest: undefined }, { digest: 'sha256:bad' }, { size: 0 }, { size: MAX_APK_BYTES + 1 }, { size: 1.5 }, { name: 'ShellfoxSetup.exe' }])('rejects unverified asset %j', override => {
    expect(parseAndroidRelease({ ...release, assets: [{ ...asset, ...override }] }, '0.8.0')).toBeNull();
  });
  it('ignores unrelated desktop/headless assets, even with no digest', () => {
    expect(parseAndroidRelease({ ...release, assets: [{ name: 'desktop.zip' }, null, asset] }, '0.8.0')).not.toBeNull();
  });
  it('compares numeric components rather than strings', () => {
    expect(isNewerVersion('0.10.0', '0.9.9')).toBe(true);
    expect(isNewerVersion('1.0.0', '0.999.999')).toBe(true);
    expect(isNewerVersion('0.9.1', '0.9.0')).toBe(true);
    expect(isNewerVersion('0.9.0', '0.10.0')).toBe(false);
  });
});
