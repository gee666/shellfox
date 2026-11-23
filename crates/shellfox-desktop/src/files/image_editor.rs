use super::{FileOperation, Preview, pending::FileAction};
use crate::root::Root;
use gpui::{Bounds, Context, Image, Pixels, RenderImage};
use image::RgbaImage;
use shellfox_files::{EditorCancellation, ErrorCode, FileError, ImageWrite};
use std::{cell::Cell, rc::Rc, sync::Arc};
mod pixels;
pub(crate) use pixels::Crop;
pub(crate) use pixels::Transform;

pub(crate) enum ImageEditor {
    Loading(Rc<()>),
    Failed(String),
    Ready(Draft),
}
pub(crate) struct Draft {
    original: Arc<RgbaImage>,
    current: Arc<RgbaImage>,
    baseline_frame: Arc<RenderImage>,
    pub frame: Arc<RenderImage>,
    pub edits: u16,
    pub working: bool,
    pub(crate) reload_required: bool,
    pub error: Option<String>,
    pub crop: Option<Crop>,
    pub anchor: Option<(u32, u32)>,
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl ImageEditor {
    pub fn dirty(&self) -> bool {
        matches!(self, Self::Ready(draft) if draft.edits > 0)
    }
    pub(crate) fn transforming(&self) -> bool {
        matches!(self, Self::Ready(draft) if draft.working)
    }

    fn ready_mut(&mut self) -> Option<&mut Draft> {
        match self {
            Self::Ready(draft) => Some(draft),
            _ => None,
        }
    }
}

impl Draft {
    fn new(pixels: RgbaImage, frame: Arc<RenderImage>) -> Self {
        let pixels = Arc::new(pixels);
        Self {
            original: pixels.clone(),
            current: pixels,
            baseline_frame: frame.clone(),
            frame,
            edits: 0,
            working: false,
            reload_required: false,
            error: None,
            crop: None,
            anchor: None,
            bounds: Rc::default(),
        }
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.current.dimensions()
    }
    fn clean_selection(&mut self) {
        self.crop = None;
        self.anchor = None;
    }
    fn reset(&mut self) {
        self.current.clone_from(&self.original);
        self.frame.clone_from(&self.baseline_frame);
        self.edits = 0;
        self.error = None;
        self.clean_selection();
    }
}

impl Root {
    pub(crate) fn toggle_image_editor(&mut self, cx: &mut Context<Self>) {
        let edit = self
            .files
            .panel
            .as_ref()
            .is_some_and(|files| files.image_editor.is_none());
        self.request_file_action(FileAction::ImageEditing(edit), cx);
    }

    pub(super) fn set_image_editing(&mut self, edit: bool, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files.panel else {
            return;
        };
        if !edit {
            files.image_editor = None;
            cx.notify();
            return;
        }
        if files.busy {
            return;
        }
        let Some(Preview::Image {
            image: Some(image), ..
        }) = &files.preview
        else {
            return;
        };
        let origin = image.clone();
        let input = origin.clone();
        let load = Rc::new(());
        files.image_editor = Some(ImageEditor::Loading(load.clone()));
        let task = cx.background_executor().spawn(async move {
            let pixels = pixels::decode(&input)?;
            let frame = pixels::frame(&pixels);
            anyhow::Ok((pixels, frame))
        });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let target = root.files.panel.iter_mut().chain(&mut root.files.retained).find(|files| {
                    matches!(&files.image_editor, Some(ImageEditor::Loading(id)) if Rc::ptr_eq(id, &load))
                        && same_image(&files.preview, &origin)
                });
                let Some(files) = target else { return };
                files.image_editor = Some(match result {
                    Ok((pixels, frame)) => ImageEditor::Ready(Draft::new(pixels, frame)),
                    Err(error) => ImageEditor::Failed(error.to_string()),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn image_draft(&mut self) -> Option<&mut Draft> {
        let files = self.files.panel.as_mut()?;
        if files.busy {
            return None;
        }
        files
            .image_editor
            .as_mut()?
            .ready_mut()
            .filter(|draft| !draft.working)
    }

    pub(crate) fn reset_image(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.image_draft() {
            draft.reset();
            cx.notify();
        }
    }

    pub(crate) fn transform_image(&mut self, action: Transform, cx: &mut Context<Self>) {
        if !self.files_source_current() {
            return;
        }
        let Some(files) = &mut self.files.panel else {
            return;
        };
        let Some(draft) = files.image_editor.as_mut().and_then(ImageEditor::ready_mut) else {
            return;
        };
        if files.busy || draft.working {
            return;
        }
        if draft.edits >= 256 {
            draft.error = Some("Save or reset before making more edits.".into());
            cx.notify();
            return;
        }
        draft.working = true;
        let current = draft.current.clone();
        let input = current.clone();
        let draft_id = draft.bounds.clone();
        let task = cx.background_executor().spawn(async move {
            let pixels = pixels::apply(&input, action)?;
            let frame = pixels::frame(&pixels);
            anyhow::Ok((pixels, frame))
        });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let target = root
                    .files
                    .panel
                    .iter_mut()
                    .chain(&mut root.files.retained)
                    .filter_map(|files| {
                        files.image_editor.as_mut().and_then(ImageEditor::ready_mut)
                    })
                    .find(|draft| {
                        draft.working
                            && Rc::ptr_eq(&draft.bounds, &draft_id)
                            && Arc::ptr_eq(&draft.current, &current)
                    });
                let Some(draft) = target else { return };
                draft.working = false;
                match result {
                    Ok((pixels, frame)) => {
                        draft.current = Arc::new(pixels);
                        draft.frame = frame;
                        draft.edits += 1;
                        draft.error = None;
                        draft.clean_selection();
                    }
                    Err(error) => draft.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn save_edited_image(&mut self, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files.panel else {
            return;
        };
        let Some(draft) = files.image_editor.as_mut().and_then(ImageEditor::ready_mut) else {
            return;
        };
        let Some(Preview::Image {
            path,
            revision,
            image: Some(image),
            ..
        }) = &files.preview
        else {
            return;
        };
        if files.busy || draft.working || draft.reload_required || draft.edits == 0 {
            return;
        }
        let path = path.clone();
        let revision = revision.clone();
        let format = image.format;
        let pixels = draft.current.clone();
        let backend = files.backend.clone();
        let worker = backend.clone();
        files.request += 1;
        let request_id = files.request;
        let cancel = EditorCancellation::default();
        files.operation = Some(FileOperation::Save(cancel.clone()));
        files.busy = true;
        draft.error = None;
        let task = cx.background_executor().spawn(async move {
            let bytes = pixels::encode(&pixels, format)?;
            let input = ImageWrite {
                path,
                revision,
                mime_type: format.mime_type().into(),
                bytes,
            };
            let written = worker.write_image(&input, &cancel).await?;
            anyhow::ensure!(
                written.path == input.path,
                "Saved image path does not match the original document."
            );
            anyhow::Ok(Preview::image(
                written.path,
                &input.mime_type,
                written.revision,
                input.bytes,
            ))
        });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let Some((files, _)) = root.file_write_recipient(request_id, &backend) else {
                    return;
                };
                files.operation = None;
                files.busy = false;
                let Some(draft) = files.image_editor.as_mut().and_then(ImageEditor::ready_mut)
                else {
                    return;
                };
                match result {
                    Ok(preview) => {
                        let mut prepared = Some(preview);
                        if let Err(error) = backend
                            .commit_if_current(|| std::mem::swap(&mut files.preview, &mut prepared))
                        {
                            draft.reload_required = true;
                            draft.error =
                                Some(format!("Image save outcome unconfirmed: {error:#}"));
                        } else {
                            draft.original.clone_from(&draft.current);
                            draft.baseline_frame.clone_from(&draft.frame);
                            draft.edits = 0;
                            draft.clean_selection();
                        }
                    }
                    Err(error) => {
                        draft.reload_required = !error
                            .downcast_ref::<FileError>()
                            .is_some_and(|error| error.code == ErrorCode::FileConflict);
                        draft.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn same_image(preview: &Option<Preview>, image: &Arc<Image>) -> bool {
    matches!(preview, Some(Preview::Image { image: Some(current), .. }) if Arc::ptr_eq(current, image))
}
