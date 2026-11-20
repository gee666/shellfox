use gpui::{App, PathPromptOptions, Task, Window};
use std::path::PathBuf;

pub(crate) fn open(
    window: &Window,
    cx: &App,
    options: PathPromptOptions,
) -> Task<anyhow::Result<Option<PathBuf>>> {
    #[cfg(all(target_os = "macos", debug_assertions, feature = "test-macos-p4"))]
    if let Some(root) = std::env::var_os("SHELLFOX_P4_PICKER_ROOT") {
        let dir = PathBuf::from(root).join(if options.directories {
            "project"
        } else {
            "upload"
        });
        return fixture_open(window, cx, dir, options);
    }
    let _ = window;
    let request = cx.prompt_for_paths(options);
    cx.foreground_executor()
        .spawn(async move { Ok(request.await??.and_then(|paths| paths.into_iter().next())) })
}

// CI supplies only the initial directory. The real panel owns selection and cancel.
#[cfg(all(target_os = "macos", debug_assertions, feature = "test-macos-p4"))]
fn fixture_open(
    window: &Window,
    cx: &App,
    dir: PathBuf,
    options: PathPromptOptions,
) -> Task<anyhow::Result<Option<PathBuf>>> {
    use anyhow::Context as _;
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponse, NSModalResponseCancel, NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSString, NSURL};
    use std::{
        cell::RefCell,
        ffi::{CStr, OsStr},
        os::unix::ffi::OsStrExt,
    };

    let owner = crate::macos_window::owner(window);
    cx.foreground_executor().spawn(async move {
        let owner = owner.context("Missing native Files picker owner")?;
        let main = MainThreadMarker::new().context("AppKit main thread unavailable")?;
        anyhow::ensure!(
            dir.is_absolute() && dir.is_dir(),
            "Invalid P4 picker directory"
        );
        let directory_url = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(dir.to_str().context("P4 picker directory is not UTF-8")?),
            true,
        );
        let panel = NSOpenPanel::openPanel(main);
        panel.setDirectoryURL(Some(&directory_url));
        panel.setCanChooseDirectories(options.directories);
        panel.setCanChooseFiles(options.files);
        panel.setAllowsMultipleSelection(false);
        if let Some(title) = options.prompt {
            panel.setTitle(Some(&NSString::from_str(&title)));
        }
        let (send, receive) = tokio::sync::oneshot::channel();
        let send = RefCell::new(Some(send));
        let result_panel = panel.clone();
        let completed = RcBlock::new(move |response: NSModalResponse| {
            let result = if response == NSModalResponseOK {
                result_panel
                    .URLs()
                    .firstObject()
                    .map(|url| {
                        // The filesystem representation is owned by this retained NSURL.
                        let bytes =
                            unsafe { CStr::from_ptr(url.fileSystemRepresentation().as_ptr()) }
                                .to_bytes();
                        PathBuf::from(OsStr::from_bytes(bytes))
                    })
                    .map(Some)
                    .context("Native Files picker returned no path")
            } else if response == NSModalResponseCancel {
                Ok(None)
            } else {
                Err(anyhow::anyhow!("AppKit could not finish the Files picker"))
            };
            if let Some(send) = send.borrow_mut().take() {
                let _ = send.send(result);
            }
        });
        panel.beginSheetModalForWindow_completionHandler(&owner, &completed);
        receive.await?
    })
}
