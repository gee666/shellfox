# Terminal NUL compatibility fix

This is an authorized functional fix to the existing terminal contracts and input routes. It does not change the Electron or mobile visual baseline, add a method or enable binary X10 input.

Ctrl+Space produces byte zero. The previous shared schema, desktop input guard, mobile page/bridge guards and active Windows helper rejected it. Terminal input now preserves NUL, including bytes after it. NUL remains invalid in paths, titles, environment values and saved file text. Clipboard paste policy is unchanged.

The affected input boundaries retain their existing controller generation/epoch, view identity, sequence, queue, frame and rate checks. Terminal data still has the existing 65,536 UTF-16-unit and 65,536 UTF-8-byte bounds in TypeScript. Rust retains the 65,536-byte bound; for valid Unicode this also bounds UTF-16 units. NUL counts as one unit and one UTF-8 byte.

## Byte ownership

The Windows bridge frames `data` as JSON and counts `Buffer.byteLength(data)`. The active native `ConPty.QueueInput` decodes it using `Wire.Utf8.GetBytes(text)` and enqueues a `byte[]`. The writer calls `FileStream.WriteAsync(write.Data)`, then acknowledges that array's explicit length. It does not pass terminal input as a NUL-terminated string to a C API. Launch arguments and native path/environment handling were not relaxed.

The native regression creates a real owned ConPTY through the existing isolated launch. It uses trusted system PowerShell with `-NoProfile`, a closed environment and worktree-`tmp/` state. A native-console probe disables cooked input and reads three bytes from stdin. Sending `A\0B` through the actual `QueueInput` returns `65,0,66`. The test confirms exit and an empty job, then deletes its scratch root. It does not start a substitute host, request elevation, read account profiles or access a network.

## Focused evidence

- Seven Vitest suites passed 180 tests, including schema, desktop registry, mobile protocol/bridge, Node IPC, approved remote HTTP and Windows helper framing.
- Offline Chromium terminal tests passed 56 checks. Real hardware and accessory Ctrl+Space each emitted one NUL message through the bundled xterm page.
- The active Windows helper's two new native tests passed. One checks framed NUL byte lengths; the other checks real raw ConPTY input and owned-process cleanup.
- The shared Rust client already tests NUL input while retaining path/environment rejection.

The retained helper artifact must be rebuilt from this source before exercising the changed behavior through that artifact. No installed production helper was replaced. These results cover the local Windows x64 environment and the offline browser. They do not establish Android, iOS, macOS or Linux native acceptance.

## Binary mouse input remains separate

The v1 input contract carries UTF-8 text and has no binary encoding flag. xterm DEFAULT/X10 mouse reports use `onBinary`, while the active clients subscribe to `onData`. Raw high-byte reports cannot be mapped to Unicode strings without changing their bytes at the host. A negotiated binary extension or supported encoding constraint still needs an architecture decision and end-to-end tests. This fix adds neither.
