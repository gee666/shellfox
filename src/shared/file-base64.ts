/** Linear validation avoids repeated-group RegExp stack overflow on large previews. */
export function canonicalBase64(value: string, allowEmpty = false): boolean {
  if ((!value.length && !allowEmpty) || value.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(value)) return false;
  if (value.endsWith('==')) return 'AQgw'.includes(value[value.length - 3]!);
  return !value.endsWith('=') || 'AEIMQUYcgkosw048'.includes(value[value.length - 2]!);
}
export function validDataUrl(value: string, maxBytes: number, imageOnly = false): boolean {
  if (value.length > Math.ceil(maxBytes / 3) * 4 + 128) return false;
  const comma = value.indexOf(','), header = value.slice(0, comma);
  if (!(imageOnly ? /^data:image\/(?:png|jpeg|webp);base64$/ : /^data:[a-zA-Z0-9.+/-]+;base64$/).test(header)) return false;
  const encoded = value.slice(comma + 1);
  return canonicalBase64(encoded, !imageOnly) && encoded.length / 4 * 3 - (encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0) <= maxBytes;
}
