//! Owner-supplied chrome before the first native paint; never read the owner's database here.
use crate::{desktop_geometry::WindowHistory, host_client::HostClient, theme::Palette};
use gpui::{App, Global};
use shellfox_service::native::{NativeAction, NativeResponse};

pub(crate) struct StartupChrome {
    history: Option<WindowHistory>,
    colors: Option<Palette>,
}

pub(crate) struct StartupPalette {
    pub(crate) palette: Palette,
    revision: Option<u64>,
}
impl Global for StartupPalette {}

impl StartupChrome {
    pub(crate) async fn read(client: HostClient) -> Self {
        let (history, settings) = tokio::join!(
            WindowHistory::read(client.clone()),
            client.execute(NativeAction::Settings(
                shellfox_host::SettingsRequest::Get {}
            )),
        );
        let history = history
            .inspect_err(|error| eprintln!("Window bounds unavailable: {error:#}"))
            .ok();
        let colors = match settings {
            Ok(NativeResponse::Settings(settings)) => Some(Palette::new(
                &settings.accent_color,
                &settings.background_color,
            )),
            _ => None,
        };
        Self { history, colors }
    }

    pub(crate) fn install(self, cx: &mut App) {
        if let Some(history) = self.history {
            cx.set_global(history);
        }
        if let Some(palette) = self.colors {
            cx.set_global(StartupPalette {
                palette,
                revision: None,
            });
        }
    }
}

impl StartupPalette {
    pub(crate) fn remember(snapshot: &shellfox_core::Snapshot, cx: &mut App) {
        // Another window can be catching up after the current owner revision.
        if cx
            .try_global::<Self>()
            .and_then(|colors| colors.revision)
            .is_some_and(|revision| revision >= snapshot.revision)
        {
            return;
        }
        let prefs = &snapshot.preferences;
        cx.set_global(Self {
            palette: Palette::new(&prefs.accent_color, &prefs.background_color),
            revision: Some(snapshot.revision),
        });
    }
}
