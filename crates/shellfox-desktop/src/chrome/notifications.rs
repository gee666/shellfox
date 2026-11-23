use crate::{assets::icon_button, root::Root, theme::Palette};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

const ERROR_DURATION: Duration = Duration::from_secs(6);
const INFO_DURATION: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Tone {
    Error,
    Info,
    AccessRequest,
}
impl Tone {
    fn duration(self) -> Duration {
        match self {
            Self::Error => ERROR_DURATION,
            Self::Info => INFO_DURATION,
            Self::AccessRequest => Duration::from_secs(12),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NoticeKey {
    key: Option<String>,
    code: Option<String>,
    tone: Tone,
    message: String,
}

#[derive(Default)]
pub(crate) struct Notifications {
    messages: VecDeque<(u64, String, Tone)>,
    recent: HashMap<NoticeKey, Instant>,
    serial: u64,
    pending_access: HashSet<String>,
}
impl Notifications {
    #[cfg(test)]
    fn push(&mut self, message: String, tone: Tone, now: Instant) -> Option<u64> {
        self.push_keyed(
            NoticeKey {
                key: None,
                code: None,
                tone,
                message,
            },
            now,
        )
    }

    fn push_keyed(&mut self, key: NoticeKey, now: Instant) -> Option<u64> {
        self.recent.retain(|_, expires| *expires > now);
        let tone = key.tone;
        let message = key.message.clone();
        if tone != Tone::AccessRequest {
            if self.recent.contains_key(&key) {
                return None;
            }
            self.recent.insert(key, now + tone.duration());
        }
        self.serial += 1;
        if self.messages.len() == 4 {
            self.messages.pop_front();
        }
        self.messages.push_back((self.serial, message, tone));
        Some(self.serial)
    }

    fn update_access<'a>(
        &mut self,
        pending: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Option<String> {
        let pending: Vec<_> = pending.into_iter().collect();
        let mut added = pending
            .iter()
            .filter(|(id, _)| !self.pending_access.contains(*id));
        let first = added.next().map(|(_, host)| *host);
        let count = added.count() + usize::from(first.is_some());
        self.pending_access = pending.into_iter().map(|(id, _)| id.to_owned()).collect();
        match (count, first) {
            (1, Some(host)) => Some(format!("{host} wants to connect")),
            (count, _) if count > 1 => Some(format!("{count} devices want to connect")),
            _ => None,
        }
    }

    fn dismiss(&mut self, id: u64) -> bool {
        let before = self.messages.len();
        self.messages.retain(|(current, _, _)| *current != id);
        self.messages.len() != before
    }
}

impl Root {
    pub(crate) fn notify_info(&mut self, cx: &mut Context<Self>, message: impl Into<String>) {
        self.push_notice(message.into(), Tone::Info, cx);
    }

    pub(crate) fn update_pending_access<'a>(
        &mut self,
        pending: impl IntoIterator<Item = (&'a str, &'a str)>,
        cx: &mut Context<Self>,
    ) {
        if let Some(message) = self.notifications.update_access(pending) {
            self.push_notice(message, Tone::AccessRequest, cx);
        }
    }

    pub(crate) fn pending_access_count(&self) -> usize {
        self.notifications.pending_access.len()
    }

    pub(super) fn collect_errors(&mut self, cx: &mut Context<Self>) {
        if let Some(error) = self.error.take() {
            self.push_notice(error, Tone::Error, cx);
        }
    }

    pub(crate) fn notify_error(
        &mut self,
        key: Option<String>,
        code: Option<String>,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.push_with_key(
            NoticeKey {
                key,
                code,
                tone: Tone::Error,
                message: message.into(),
            },
            cx,
        );
    }

    pub(crate) fn notify_machine_error(&mut self, message: &str, cx: &mut Context<Self>) {
        self.notify_error(None, None, machine_notice_line(message), cx);
    }

    pub(crate) fn notify_operation_error(&mut self, error: &anyhow::Error, cx: &mut Context<Self>) {
        if let Some(error) = error.downcast_ref::<shellfox_core::Error>() {
            self.notify_error(None, error_code(error.code), error.message.clone(), cx);
        } else if let Some(failure) =
            error.downcast_ref::<shellfox_service::local::LocalRequestFailure>()
        {
            self.notify_error(
                None,
                error_code(failure.error.code),
                failure.error.message.clone(),
                cx,
            );
        } else {
            self.notify_error(None, None, format!("Host operation failed: {error:#}"), cx);
        }
    }

    fn push_notice(&mut self, message: String, tone: Tone, cx: &mut Context<Self>) {
        self.push_with_key(
            NoticeKey {
                key: None,
                code: None,
                tone,
                message,
            },
            cx,
        );
    }

    fn push_with_key(&mut self, notification: NoticeKey, cx: &mut Context<Self>) {
        let tone = notification.tone;
        let Some(id) = self.notifications.push_keyed(notification, Instant::now()) else {
            return;
        };
        cx.spawn(async move |root, cx| {
            cx.background_executor().timer(tone.duration()).await;
            let _ = root.update(cx, |root, cx| {
                if root.notifications.dismiss(id) {
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_toasts(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let width = (window.viewport_size().width - px(24.)).min(px(360.));
        div()
            .absolute()
            .right(px(12.))
            .bottom(px(12.))
            .max_w(width)
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(
                self.notifications
                    .messages
                    .iter()
                    .map(|(id, message, tone)| {
                        let id = *id;
                        let info = *tone != Tone::Error;
                        let mut body = div()
                            .min_w_0()
                            .flex_1()
                            .min_h(px(28.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .child(message.clone());
                        body.style().align_self = Some(gpui::AlignSelf::Center);
                        div()
                            .id(gpui::SharedString::from(format!("error-toast-{id}")))
                            .min_w_0()
                            .relative()
                            .occlude()
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .py(px(8.))
                            .pr(px(8.))
                            .pl(px(13.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .bg(palette.panel)
                            .border_1()
                            .border_color(palette.border)
                            .rounded(px(6.))
                            .overflow_hidden()
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.4),
                                offset: gpui::point(px(0.), px(8.)),
                                blur_radius: px(24.),
                                spread_radius: px(0.),
                            }])
                            .when(info, |toast| toast.text_color(palette.muted))
                            .child(div().absolute().top_0().bottom_0().left_0().w(px(2.)).bg(
                                if info {
                                    palette.border
                                } else {
                                    palette.status_color(0xfc8397)
                                },
                            ))
                            .child(self.semantic_region(
                                &format!("chrome/notice/{id}"),
                                if info {
                                    crate::accessibility::Role::Status
                                } else {
                                    crate::accessibility::Role::Alert
                                },
                                message.clone(),
                                body,
                            ))
                            .when(*tone == Tone::AccessRequest, |toast| {
                                let mut review = div()
                                    .id(("review-access", id as usize))
                                    .ml(px(4.))
                                    .px(px(7.))
                                    .py(px(3.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(palette.border)
                                    .bg(palette.surface)
                                    .text_size(px(11.))
                                    .text_color(palette.accent)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(palette.hover))
                                    .child("Review")
                                    .on_click(cx.listener(move |root, _, window, cx| {
                                        root.review_access_requests(window, cx);
                                        root.notifications.dismiss(id);
                                        cx.notify();
                                    }));
                                review.style().align_self = Some(gpui::AlignSelf::Center);
                                toast.child(review)
                            })
                            .child(
                                icon_button(format!("dismiss-error-{id}"), "close", palette)
                                    .on_click(cx.listener(move |root, _, _, cx| {
                                        if root.notifications.dismiss(id) {
                                            cx.notify();
                                        }
                                    })),
                            )
                    }),
            )
            .into_any_element()
    }
}

fn machine_notice_line(message: &str) -> &str {
    let line = message.lines().next().unwrap_or_default();
    let mut utf16 = 0;
    let end = line
        .char_indices()
        .find_map(|(index, character)| {
            utf16 += character.len_utf16();
            (utf16 > 200).then_some(index)
        })
        .unwrap_or(line.len());
    &line[..end]
}

fn error_code(code: impl serde::Serialize) -> Option<String> {
    serde_json::to_value(code).ok()?.as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests;
