import { expect, it } from 'vitest';
import { FILE_INLINE_IMAGE_WRITE_BYTES, FILE_INLINE_TEXT_BYTES } from '../../shared/file-schemas';
import { decodeImage, decodeText } from './content';

it('keeps legacy text decoding bounded at 128 KiB independently of streamed preview limits', () => {
  expect(decodeText(Buffer.alloc(FILE_INLINE_TEXT_BYTES, 97))).toHaveLength(FILE_INLINE_TEXT_BYTES);
  expect(() => decodeText(Buffer.alloc(FILE_INLINE_TEXT_BYTES + 1, 97))).toThrow('128 KiB');
});
it('keeps legacy image decoding bounded at 512 KiB with an accurate error message', () => {
  const image = Buffer.alloc(FILE_INLINE_IMAGE_WRITE_BYTES + 1);
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]).copy(image);
  expect(decodeImage(`data:image/png;base64,${image.subarray(0, FILE_INLINE_IMAGE_WRITE_BYTES).toString('base64')}`, 'image.png')).toHaveLength(FILE_INLINE_IMAGE_WRITE_BYTES);
  expect(() => decodeImage(`data:image/png;base64,${image.toString('base64')}`, 'image.png')).toThrow('512 KiB');
});
