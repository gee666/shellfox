# Files, previews and transfers

## Using the panel

The folder icon opens or hides Files. It works without a running terminal or any saved sessions. The initial folder is the selected session's working directory when available, otherwise the selected host's home directory. A terminal file link opens its target in the same panel.

The chevron inside the panel collapses only the folder browser. The current file stays open. Drag the sidebar's left edge to change its width, or the divider between the browser and document to change their heights. Both handles support keyboard resizing and remember their sizes.

The maximize icon fills the Electron app's content area without entering operating-system fullscreen. It keeps the same editor and HTML iframe alive, so drafts and interactive page state survive. Restore returns the previous layout. The copy-path icon copies the opened file's canonical host path to the viewing device's clipboard.

The separate terminal-folder icon resolves the selected terminal's current directory and opens the browser there, even if it was hidden or browsing another folder. It does not discard the open file's draft. The path entry is shared with the new-session working-directory picker. Enter accepts a suggestion while the list is open. Escape dismisses the list without changing the entered path; Enter then opens that path, including the home directory. Another Escape closes the enclosing picker. Clicking or editing the entry opens suggestions again.

Text has a wrap toggle that changes display, not file contents. Images have a single row of compact rotation, flip, crop and reset icons, with Save last. Drag or touch the image to select a rectangle, then use Crop. There are no numeric crop fields.

## Mobile navigation

Mobile uses separate full-screen browser and file pages. A file has one Close action, returning to its previous browser page or its terminal caller. The browser has one Back action. Closing a file restores the preceding folder snapshot and scroll position, including while disconnected; it does not need another folder request.

Only one compact icon toolbar is permanent. The filename uses the remaining width. Read mode offers editing, path, copy, download and More. Text edit mode keeps Save at the right; secondary actions such as wrapping and reloading live in More. Switching text into read/preview mode retains the unsaved draft, which can still be saved. Image editing uses its own single toolbar with Close and More before the image actions and Save last.

The path field is hidden until its icon is selected. Path and More sheets do not recreate the file editor or HTML view. Copy always uses the viewing phone's clipboard and the scoped host path. Transfers and errors use temporary overlays instead of reserving another navigation row. The browser list uses full-width rows.

The emulator UI demo opens the browser directly. Internal native-test controls are in a separate QA-only entry point, not on the application's navigation path.

## What happens on a remote host

File operations run on the selected Shellfox desktop or headless host. Its operating-system account and file permissions apply. The viewing app does not resolve remote paths against its own filesystem.

Opening a preview transfers the file's contents to the viewing device over the approved Shellfox connection. Text is decoded on that device. Images are decoded there too. For HTML and Markdown, the app requests allowed sibling assets from the host and embeds them in an isolated local document. HTML JavaScript runs on the viewing device, not on the remote host. It cannot use Shellfox's native bridge, run host commands, fetch arbitrary remote files or make network requests.

Saving sends edited contents back to the host. The host compares the file's current SHA-256 revision with the revision read by the editor. A changed file produces `FILE_CONFLICT`; it is not silently overwritten. Writes are staged beside the destination, synced and committed after validation. Transfers do not retry uncertain writes automatically.

### Preview memory is not a downloaded file

- A normal preview does not create an ordinary file in Downloads. Its data is held in the viewing app's memory.
- The current preview and directory listings can remain while their panel stays mounted, including when hidden or when switching sessions. There is no fixed preview-cache TTL. Reload/refresh requests current host data.
- Opening another file replaces the clean preview. Memory becomes eligible for collection when no active view or draft retains it.
- Unsaved text drafts remain in memory until saved, discarded or the app process exits. Image operation histories are retained for up to eight open drafts, scoped to the filesystem connection. They are not shared between hosts with identical paths and contents.
- Drafts are not a durable recovery journal. Force-quitting or an app crash can lose unsaved edits. Disconnecting does not automatically discard a mounted editor's draft.
- Mobile file WebViews disable normal web caching and use private/incognito settings. Desktop preview documents are served with `Cache-Control: no-store`. This describes application behavior, not a guarantee about operating-system swap or crash diagnostics.

Explicit Download is different: it saves a lasting device file. Shellfox does not delete completed downloads when the preview closes. Upload reads a file chosen through the local device's picker; the chosen original is not modified. An operating-system document provider may supply a temporary copy, which the app releases when the operation ends.

## Upload and download

The upload icon sends a selected local file to the browser's current host folder. It captures that host and folder when clicked, so changing navigation cannot redirect an in-flight upload. Create-mode uploads do not overwrite an existing filename.

The download icon saves the selected host file, including files that are too large or unsupported for an inline preview. Desktop uses the native save dialog with Downloads as the default. Android uses MediaStore Downloads on Android 10 and newer, publishing the item only after a complete write; older Android versions use the system create-document picker. iOS exports through the Files document picker because it has no app-writable global Downloads directory.

Transfers use sequential 192 KiB chunks. File transfer data is not assembled as one giant JSON message or one complete JavaScript download buffer. Cancellation, disconnects, expiry and owner destruction clean up open handles and incomplete staged writes. The host records known `.shellfox-transfer-<UUID>.partial` stages in a bounded recovery journal at `<userData>/file-transfer-stages.json`. After a crash it checks canonical parents and file identities before removing known incomplete stages. It never scans arbitrary folders or removes completed destination files; uncertain identity mismatches remain untouched for safety. These are temporary upload stages, not a preview cache. A file changed during a read or replacement is reported rather than accepted as a consistent snapshot.

## Why there are still safeguards

The original 2 MiB preview and 512 KiB image-save caps were Shellfox implementation choices, not T3 Code requirements. The image cap kept base64-encoded data inside the old 1 MiB request envelope. Chunked transport removes that bottleneck without increasing the maximum size of ordinary RPC messages.

The current safeguards distinguish editing from transfer:

| Operation | Limit |
| --- | --- |
| Text preview/editing | 8 MiB of UTF-8 text |
| Raster image preview/editing and each preview asset | 64 MiB |
| Combined embedded HTML assets | 8 MiB of data URLs, at most 100 assets |
| Encoded desktop HTML preview document | 12 MiB; larger documents remain editable/downloadable |
| Upload/download | 256 MiB by default, configurable up to 1 GiB |
| Transfer chunk | 192 KiB |
| Open host transfers | 2 per owner, 8 per host |
| Idle transfer lease | 5 minutes; swept every 30 seconds |
| Image editing dimensions | 8,192 per edge and 16 million pixels |

Set `SHELLFOX_FILE_TRANSFER_MAX_BYTES` on the host before startup to change the transfer ceiling. Its accepted range is 1 byte through 1 GiB. Preview/editor memory guards remain independent: a file can be downloadable even if it is not suitable for an inline editor. Image export preserves PNG, JPEG or WebP rather than silently changing format.

Unbounded image decoding or textarea contents can exhaust a phone's memory even when the compressed file is small. The image bootstrap document therefore stays small; source bytes are delivered through the editor's validated message channel rather than encoded into an enormous document URL.

T3 Code also has per-viewer limits. In the inspected upstream revision, the ordinary project reader reads up to 1 MiB, its large-text viewer supports up to 256 MiB, fonts have a 10 MiB cap, and several Office/binary previews have a 50 MiB cap. Its large-text implementation is not an unrestricted editable textarea.

## Contracts and paths

Production preload, attached hosts and approved remote peers expose `ManagerApi.files`. The property is optional for older injected clients. Requests and responses use strict Zod schemas.

`FileLocation` contains `path`, optional `sessionId` and optional `tabId`. A tab requires its matching session. Without a session, relative paths use the selected host's home directory. Terminal-linked relative paths use the owned shell's current directory when available. Linux and WSL query the authenticated shell through `/proc`; macOS uses a bounded `lsof` query with supervisor identity checks. New normal PowerShell tabs install a reporting hook that delegates the user's prompt without editing profiles or writing commands into PTY input.

Existing pre-hook Windows tabs may need reopening before Current directory can work. Elevated PowerShell does not yet have a startup-hook channel; its current folder is unavailable unless its prompt already emits OSC 7. The explicit current-folder action reports this limitation instead of calling the launch folder current. Old relative file links keep the launch/report fallback when a live directory cannot be obtained; absolute file paths remain usable.

Terminal file clicks are handled by Shellfox even while an agent/TUI captures mouse input. A click snapshots its target before the next output repaint and is not also sent to the terminal application. Web-link handling is unchanged. File URI source locations such as `#L12C3` and `:12:3` open the corresponding source position.

Windows WSL guest paths retain their originating session/tab context for reads, writes, assets and transfers. They are never reinterpreted as ordinary Windows paths. If that context no longer exists, the operation fails explicitly; returning to host home recovers ordinary browsing. Direct nested SSH/SFTP browsing is not implemented. Connect that machine as a Shellfox host instead.

Existing convenience methods are `list`, `read`, `write`, `writeImage` and `readAsset`. Production clients implement the latter four through the shared streaming adapter. Legacy single-message endpoints remain small for older callers; they are not the production preview transport.

Streaming methods are:

- `beginRead(location & {assetPath?})`: opens a read lease and returns metadata plus `transferId`.
- `readChunk({transferId, offset})`: returns base64 data, the next offset and EOF.
- `beginWrite(location & {size, mode, revision?, content, mimeType?})`: reserves a staged create or revision-checked replacement.
- `writeChunk({transferId, offset, data})`: accepts the next sequential chunk and returns the next offset.
- `finishWrite({transferId})`: validates size/content/authorization and commits.
- `cancelTransfer({transferId})`: releases a lease and any incomplete stage.

Handles are opaque and bound to their authenticated owner. Another peer cannot use a guessed or copied handle. Approval is rechecked before committing remote writes. Transfer handles expire after inactivity; the client normally cancels read handles immediately after completion rather than waiting for expiry.

Preview assets remain canonically confined below the HTML/Markdown file's parent directory. Traversal and symlinks escaping that directory fail. Assets use a MIME allowlist; an HTML document is not granted a public filesystem server. HTML sandboxing omits same-origin, popups, downloads and native APIs. ES module imports and fetch-based applications are not a full-browser replacement.

## T3 Code reuse

Exact upstream source paths and adaptations are in `src/renderer/files/reuse.md`. The full MIT notice is in `src/renderer/files/T3CODE-LICENSE.txt` and is included in desktop build output. Shellfox retains its own host routing, strict schemas, native transfer code and explicit-save editor lifecycle.
