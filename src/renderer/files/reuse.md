# T3 Code reuse

Source: https://github.com/pingdotgg/t3code/tree/67eebaca23f8f7bc46e8817434087fc9c5acf8e3

T3 Code is MIT licensed, copyright 2026 T3 Tools Inc. The complete notice is in `T3CODE-LICENSE.txt` beside this file.

Direct reuse and adaptations:

- `file-preview-utils.ts`: `isMarkdownPreviewFile` from `apps/web/src/components/files/filePreviewMode.ts`, unchanged except formatting.
- `file-preview-utils.ts`: `LEADING_FRONTMATTER` and `maskMarkdownFrontmatter` from `packages/shared/src/markdownFrontmatter.ts`, unchanged behavior.
- `file-preview-utils.ts`: directory-first, numeric, case-insensitive sorting adapted from `fileBreadcrumbChildren` in `apps/web/src/components/files/filePath.ts`. Shellfox sorts backend-provided names rather than extracting relative paths.
- `FilePreview.tsx`: opaque-origin iframe sandbox and stable document mounting adapted from `FileDocument` in `apps/web/src/components/files/BrowserDocumentFrame.tsx`. Shellfox deliberately omits popup/download permissions.

Inspected `FileBrowserPanel.tsx`, `FileExplorerPane.tsx`, `FilePreviewPanel.tsx`, `ReadOnlySourcePreview.tsx`, `useDirectoryEntries.ts`, `fileEditorDismissal.ts`, `fileTreeExpansion.ts`, and `fileSaveCoordinator.ts`. These depend on T3 Code's Effect state, RPC, signed assets, Pierre tree/diff workers, annotations, and application settings. Copying them would bring unrelated infrastructure into Shellfox. The sidebar tree, draft retention, explicit-save editor, and loading/error handling are Shellfox implementations. The editor uses Prism and a native textarea; Markdown uses react-markdown and remark-gfm.
