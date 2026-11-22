use super::*;
use crate::options::{Mode, Options};

impl Root {
    pub fn open(
        options: Options,
        initial: crate::window_manager::InitialSelection,
        native: Result<
            crate::accessibility::NativeAccessibility,
            crate::accessibility::SemanticError,
        >,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut root = Self {
            host: None,
            tab_activity: Default::default(),
            peers: crate::root::source::PeerSources::default(),
            peer_ui: crate::root::peer::PeerUi::default(),
            semantic_context: crate::accessibility::WindowSemanticContext::new(
                crate::accessibility::SemanticId(1),
            ),
            native_accessibility: None,
            pending_boot: None,
            app_host: None,
            window_id: None,
            close_confirmation: None,
            quit_focus: QuitFocus::new(cx),
            window_tracker: None,
            close_ticket: None,
            update_restart: false,
            update_ui: Default::default(),
            close_check_pending: false,
            close_failed: false,
            snapshot: None,
            navigation: crate::window_manager::WindowSelection::new(initial),
            tab_scroll: crate::chrome::TabScroll::default(),
            pending_recent: None,
            activating_session: None,
            views: HashMap::new(),
            attaching: HashSet::new(),
            failures: HashMap::new(),
            settings_panel: None,
            files: None,
            html_preview: crate::files::text_preview::html::Surface::new(&options.run_dir),
            retained_files: Vec::new(),
            fixture: None,
            run_dir: options.run_dir,
            window_caption: crate::desktop_windows::caption(cx).into(),
            error: None,
            notifications: Default::default(),
            dialog: None,
            settings: false,
            sidebar: true,
            layout_focus: cx.focus_handle(),
            sidebar_width: 220.,
            resizing_sidebar: false,
            local_expanded: true,
            history_page_size: 10,
            session_pages: 1,
            sidebar_ui: crate::sidebar::SidebarUi::new(cx),
            session_menu: None,
            tab_menu: None,
            modal_focus: cx.focus_handle(),
            closing_window: false,
            action_pending: false,
            user_action_pending: false,
            background: Default::default(),
            profiles: Vec::new(),
            catalog_epoch: 0,
            profile_menu: None,
            power: power::PowerUi::default(),
            watches: Vec::new(),
        };
        root.sidebar_ui.visibility.trace = options.ui_acceptance;
        match native {
            Ok(provider) => root.native_accessibility = Some(provider),
            Err(error) => {
                root.error = Some(format!("Native accessibility attachment failed: {error}"));
                return root;
            }
        }
        Self::start_poll(window, cx);
        root.watches.extend(root.watch_updates(window, cx));
        root.watches.extend(root.watch_semantics(window, cx));
        if options.mode == Mode::Fixture {
            match TerminalView::open(Mode::Fixture, &root.run_dir, window, cx) {
                Ok(view) => root.fixture = Some(view),
                Err(error) => root.error = Some(error.message),
            }
            return root;
        }
        // GPUI dispatches unfocused keys at its own root, above our capture handler.
        root.layout_focus.focus(window);
        cx.on_focus_lost(window, |root, window, _| root.layout_focus.focus(window))
            .detach();
        let Some(config) = options.endpoint else {
            root.error = Some(
                "The shared host requires --helper PATH and --helper-sha256 TRUSTED_DIGEST.".into(),
            );
            return root;
        };
        if config.data_dir != options.data_dir {
            root.error = Some("Endpoint and workspace data directories must match.".into());
            return root;
        }
        root.pending_boot = Some(config);
        root
    }
    pub(crate) fn start_services(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self;
        let Some(_config) = root.pending_boot.take() else {
            return;
        };
        match crate::app_host::adapter::get(cx).ok_or_else(|| anyhow::anyhow!("The exact prestarted application owner is not installed; Root will not start another Host")) {
            Ok(app_host) => {
                root.app_host = Some(app_host.clone());
                cx.spawn_in(window, async move |root, cx| {
                    let ready = app_host.open_window().await;
                    let catalog = match &ready {
                        Ok(window) => window.client.terminal_profiles().await,
                        Err(_) => Err(anyhow::anyhow!("Owner catalog is unavailable before a window client is ready")),
                    };
                    let _ = root.update_in(cx, |root, window, cx| {
                        match ready {
                            Ok(registered) => {
                                if !app_host.is_window_available(registered.id).unwrap_or(false) {
                                    root.error = Some(
                                        "Application close superseded this window startup.".into(),
                                    );
                                    cx.notify();
                                    return;
                                }
                                root.window_id = Some(registered.id);
                                let client = registered.client;
                                let panel =
                                    crate::settings::SettingsPanel::new(client.clone(), window, cx);
                                match crate::device_prefs::get(cx).and_then(|store| panel.update(cx, |panel, cx| panel.install_device_colors(store, cx))) {
                                    Ok(()) => {}
                                    Err(error) => root.error = Some(format!("Device color storage unavailable: {error}")),
                                }
                                cx.subscribe_in(&panel, window, |root, _, event, window, cx| {
                                    match event {
                                        crate::settings::SettingsEvent::Saved(settings) => {
                                            let size = settings.history_page_size as usize;
                                            root.apply_history_page_size(size);
                                            root.refresh_snapshot(window, cx);
                                            root.refresh_catalog(window, cx);
                                        }
                                        crate::settings::SettingsEvent::BackgroundSaveFailed(error) => root.notify_operation_error(error, cx),
                                        crate::settings::SettingsEvent::CloseRequested => {
                                            root.hide_settings(cx);
                                            root.focus_selected(window, cx);
                                            cx.notify();
                                        }
                                    }
                                })
                                .detach();
                                root.settings_panel = Some(panel);
                                crate::launch_runtime::install(cx);
                                root.watch_tab_activity(client.clone(), window, cx);
                                let workspace = root.watch_workspace(client.clone(), window, cx);
                                let power = root.watch_power(client.clone(), window, cx);
                                root.watches.extend([workspace, power]);
                                crate::update_view::read_initial_preference(client.clone(), cx);
                                root.host = Some(client);
                                root.refresh_snapshot(window, cx);
                            }
                            Err(error) => {
                                root.error = Some(format!("Host startup failed: {error:#}"))
                            }
                        }
                        root.accept_catalog(catalog, cx);
                    });
                })
                .detach();
            }
            Err(error) => root.error = Some(format!("Host startup failed: {error:#}")),
        }
    }
}
