use gpui::{App, Task, Window};
use std::path::PathBuf;

#[cfg(windows)]
pub(super) fn choose(window: &Window, cx: &App) -> Task<anyhow::Result<Option<PathBuf>>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let parent = HasWindowHandle::window_handle(window)
        .ok()
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
            _ => None,
        });
    cx.foreground_executor().spawn(async move {
        select_folder(parent.ok_or_else(|| anyhow::anyhow!("Missing native folder picker owner"))?)
    })
}

#[cfg(windows)]
fn select_folder(parent: isize) -> anyhow::Result<Option<PathBuf>> {
    use windows::{
        Win32::{
            Foundation::{ERROR_CANCELLED, HWND},
            System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
            UI::Shell::{
                FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
                IFileOpenDialog, SIGDN_FILESYSPATH,
            },
        },
        core::{HRESULT, HSTRING},
    };
    // The GPUI foreground executor runs on the window's COM apartment, the
    // same one used by GPUI's native picker. The scope owns the returned path.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(FOS_PICKFOLDERS | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM)?;
        dialog.SetTitle(&HSTRING::from("Choose default working directory"))?;
        if let Err(error) = dialog.Show(Some(HWND(parent as *mut _))) {
            if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(None);
            }
            return Err(error.into());
        }
        let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let value = path.to_string();
        CoTaskMemFree(Some(path.0.cast()));
        Ok(Some(PathBuf::from(value?)))
    }
}

#[cfg(target_os = "linux")]
pub(super) fn choose(window: &Window, cx: &App) -> Task<anyhow::Result<Option<PathBuf>>> {
    use ashpd::{WindowIdentifier, desktop::file_chooser::OpenFileRequest};
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    let parent = HasWindowHandle::window_handle(window).ok().map(|handle| handle.as_raw());
    let display = HasDisplayHandle::display_handle(window).ok().map(|handle| handle.as_raw());
    cx.foreground_executor().spawn(async move {
        let owner = match parent {
            Some(parent) => WindowIdentifier::from_raw_handle(&parent, display.as_ref()).await,
            None => None,
        };
        let request = OpenFileRequest::default()
            .identifier(owner)
            .modal(true)
            .title("Choose default working directory")
            .directory(true)
            .multiple(false)
            .send()
            .await?;
        match request.response() {
            Ok(response) => response
                .uris()
                .first()
                .map(|uri| {
                    uri.to_file_path().map_err(|_| {
                        anyhow::anyhow!("Native picker did not return a local directory")
                    })
                })
                .transpose(),
            Err(ashpd::Error::Response(_)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    })
}

#[cfg(target_os = "macos")]
#[path = "macos_folder_panel.rs"]
mod macos_panel;

#[cfg(target_os = "macos")]
pub(super) fn choose(window: &Window, cx: &App) -> Task<anyhow::Result<Option<PathBuf>>> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponse, NSModalResponseCancel, NSModalResponseOK};
    use std::{
        cell::RefCell,
        ffi::{CStr, OsStr},
        os::unix::ffi::OsStrExt,
    };

    let owner = crate::macos_window::owner(window);
    cx.foreground_executor().spawn(async move {
        let owner = owner.ok_or_else(|| anyhow::anyhow!("Missing native folder picker owner"))?;
        let main_thread = MainThreadMarker::new()
            .ok_or_else(|| anyhow::anyhow!("AppKit main thread unavailable"))?;
        let panel = macos_panel::folder_panel(main_thread);
        let (send, receive) = tokio::sync::oneshot::channel();
        let send = RefCell::new(Some(send));
        let result_panel = panel.clone();
        let completion = RcBlock::new(move |response: NSModalResponse| {
            let result = if response == NSModalResponseOK {
                result_panel
                    .URLs()
                    .firstObject()
                    .map(|url| {
                        // NSURL owns this NUL-terminated filesystem representation.
                        let bytes =
                            unsafe { CStr::from_ptr(url.fileSystemRepresentation().as_ptr()) }
                                .to_bytes();
                        PathBuf::from(OsStr::from_bytes(bytes))
                    })
                    .map(Some)
                    .ok_or_else(|| anyhow::anyhow!("Native picker did not return a folder"))
            } else if response == NSModalResponseCancel {
                Ok(None)
            } else {
                Err(anyhow::anyhow!("AppKit could not finish the folder picker"))
            };
            if let Some(send) = send.borrow_mut().take() {
                let _ = send.send(result);
            }
        });
        panel.beginSheetModalForWindow_completionHandler(&owner, &completion);
        receive.await?
    })
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn choose(_window: &Window, cx: &App) -> Task<anyhow::Result<Option<PathBuf>>> {
    let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.foreground_executor()
        .spawn(async move { Ok(picker.await??.and_then(|paths| paths.into_iter().next())) })
}
