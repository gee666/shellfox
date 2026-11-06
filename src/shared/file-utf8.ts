/** Strict UTF-8 independent of TextDecoder options unsupported by native polyfills.
 * File bytes retain their BOM and NUL characters; the file schema rejects NUL text. */
export function decodeFileUtf8(bytes: Uint8Array): string {
  const chunks: string[] = [], units: number[] = [];
  const invalid = () => { throw new TypeError('File is not valid UTF-8 text.'); };
  const continuation = (offset: number): number => {
    const value = bytes[offset]; if (value === undefined || value < 0x80 || value > 0xbf) return invalid(); return value;
  };
  for (let offset = 0; offset < bytes.length;) {
    const first = bytes[offset++]!; let code: number;
    if (first < 0x80) code = first;
    else if (first >= 0xc2 && first <= 0xdf) code = (first & 31) << 6 | continuation(offset++) & 63;
    else if (first >= 0xe0 && first <= 0xef) {
      const second = continuation(offset++), third = continuation(offset++);
      if (first === 0xe0 && second < 0xa0 || first === 0xed && second >= 0xa0) return invalid();
      code = (first & 15) << 12 | (second & 63) << 6 | third & 63;
    } else if (first >= 0xf0 && first <= 0xf4) {
      const second = continuation(offset++), third = continuation(offset++), fourth = continuation(offset++);
      if (first === 0xf0 && second < 0x90 || first === 0xf4 && second > 0x8f) return invalid();
      code = (first & 7) << 18 | (second & 63) << 12 | (third & 63) << 6 | fourth & 63;
    } else return invalid();
    if (code <= 0xffff) units.push(code);
    else { code -= 0x10000; units.push(0xd800 + (code >> 10), 0xdc00 + (code & 1023)); }
    if (units.length >= 8192) { chunks.push(String.fromCharCode(...units)); units.length = 0; }
  }
  if (units.length) chunks.push(String.fromCharCode(...units));
  return chunks.join('');
}

/** TextEncoder polyfills may drop lone surrogates rather than replacing them. Never save either form. */
export function isWellFormedFileText(value: string): boolean {
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const low = value.charCodeAt(++index); if (!(low >= 0xdc00 && low <= 0xdfff)) return false;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return false;
  }
  return true;
}
