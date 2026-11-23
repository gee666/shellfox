use super::{Control, control};
use crate::{
    editor_composition::EditorOutcome,
    files::{Files, transfer::TransferDirection},
    root::Root,
    theme::Palette,
};
use gpui::{Context, div, prelude::*, px};

pub(super) fn inline(
    root: &Root,
    files: &Files,
    upload_only: bool,
    palette: Palette,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let available = matches!(
        cx.try_global::<EditorOutcome>(),
        Some(EditorOutcome::Ready(_))
    );
    let active = files.transfer.as_ref();
    let can_start = available && active.is_none() && files.listing.is_some();
    div()
        .flex()
        .items_center()
        .min_w_0()
        .gap(px(3.))
        .child(Control::Upload.wrap(
            root, "Upload file", can_start, None,
            control("file-upload", "file-upload", palette)
                .when(!can_start, |button| button.opacity(0.4))
                .tooltip(crate::assets::tooltip(
                    if available {
                        "Upload a local file to this folder"
                    } else {
                        "File transfers are not supported by this host"
                    },
                    palette,
                ))
                .on_click(cx.listener(|root, _, window, cx| {
                    root.begin_file_transfer(TransferDirection::Upload, window, cx)
                })),
        ))
        .when(!upload_only, |row| {
            row.child(Control::Download.wrap(
                root, "Download selected file", can_start && files.document_path.is_some(), None,
                control("file-download", "file-download", palette)
                    .when(!can_start || files.document_path.is_none(), |button| {
                        button.opacity(0.4)
                    })
                    .tooltip(crate::assets::tooltip("Download selected file", palette))
                    .on_click(cx.listener(|root, _, window, cx| {
                        root.begin_file_transfer(TransferDirection::Download, window, cx)
                    })),
            ))
        })
        .when_some(active, |row, transfer| {
            let state = transfer.progress();
            let ratio = (state.offset as f32 / state.total.max(1) as f32).clamp(0., 1.);
            let cancelled = transfer.cancel.is_cancelled();
            let status = if cancelled {
                "Cancelling…".to_owned()
            } else if state.name.is_empty() {
                "Choosing file…".to_owned()
            } else {
                format!(
                    "{} {}",
                    if transfer.direction == TransferDirection::Upload {
                        "Uploading"
                    } else {
                        "Downloading"
                    },
                    state.name
                )
            };
            row.child(
                div()
                    .id("file-transfer-progress")
                    .w(px(24.))
                    .h(px(8.))
                    .rounded(px(2.))
                    .bg(palette.surface)
                    .overflow_hidden()
                    .tooltip(crate::assets::tooltip(status, palette))
                    .child(div().h_full().w(px(24. * ratio)).bg(palette.accent)),
            )
            .child(Control::CancelTransfer.wrap(
                root, "Cancel transfer", !cancelled, None,
                super::text_button("cancel-file-transfer", "×", palette)
                    .when(cancelled, |button| button.opacity(0.4))
                    .tooltip(crate::assets::tooltip("Cancel transfer", palette))
                    .on_click(cx.listener(|root, _, _, cx| root.cancel_file_transfer(cx))),
            ))
        })
}
