use crate::{field::Field, host_client::HostClient, theme::Palette};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Render, Subscription, Window, div,
    prelude::*, px,
};
use shellfox_core::{Settings, SettingsPatch};
use shellfox_service::native::{NativeAction, NativeResponse};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
mod agents;
pub mod api;
mod follow_up;
mod browser;
mod catalog_validation;
mod color_picker;
mod colors;
mod compat;
mod controller;
mod controls;
mod device_colors;
pub(crate) mod environment_data;
mod environment_keys;
mod environment_rpc;
mod environment_view;
mod errors;
pub use device_colors::DeviceColors;
mod device_color_actions;
mod field_semantics;
mod forms;
mod general;
mod key_picker;
mod keyboard;
mod layout;
mod login;
mod native_gateway;
pub(crate) mod native_services;
mod native_ssh;
#[cfg(test)]
mod native_ssh_tests;
pub(crate) mod native_state;
#[cfg(test)]
mod native_tests;
mod operation_tracker;
mod operations;
mod profile_keys;
mod progress;
mod remote;
mod remote_actions;
mod remote_login_view;
mod remote_page;
mod remote_peers;
mod remote_peer_row;
mod remote_poll;
pub(crate) mod pairing;
mod rule_edits;
mod rules;
mod semantic_control;
mod semantic_element;
mod semantic_enabled;
#[cfg(test)]
mod semantic_tests;
mod semantics;
mod ssh;
mod ssh_form;
mod ssh_page;
mod state;
mod terminals;
pub use api::SettingsServices;
use forms::{Forms, Input};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Category {
    General,
    Terminals,
    Agents,
    RemoteAccess,
    Ssh,
}
impl Category {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Terminals,
        Self::Agents,
        Self::RemoteAccess,
        Self::Ssh,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Terminals => "Terminals",
            Self::Agents => "Agents",
            Self::RemoteAccess => "Remote access",
            Self::Ssh => "SSH",
        }
    }
}
pub enum SettingsEvent {
    Saved(Settings),
    CloseRequested,
    BackgroundSaveFailed(anyhow::Error),
}
/// Settings in a window: whether the page is shown, and its panel once created.
#[derive(Default)]
pub(crate) struct SettingsSlot {
    pub(crate) open: bool,
    pub(crate) panel: Option<Entity<SettingsPanel>>,
}

pub struct SettingsPanel {
    client: HostClient,
    operations: operations::Operations,
    semantic: Option<semantics::Binding>,
    semantic_actions: BTreeMap<crate::accessibility::SemanticId, semantics::Invocation>,
    semantic_fields: BTreeMap<crate::accessibility::SemanticId, Entity<Field>>,
    draft: Option<state::Draft>,
    category: Category,
    palette: Palette,
    forms: Forms,
    subscriptions: Vec<Subscription>,
    loading: bool,
    load_error: Option<String>,
    saved_visible: bool,
    save_error: Option<(errors::Field, String)>,
    save_ready: bool,
    visible: bool,
    saved_epoch: u64,
    agent_error: Option<String>,
    environment: Option<forms::Environment>,
    environment_subscription: Option<Subscription>,
    restore_environment_focus: bool,
    environment_write: Option<environment_rpc::PendingEnvironment>,
    environment_writing: bool,
    catalog: Option<api::Catalog>,
    catalog_error: Option<String>,
    profile_menu: bool,
    profile_index: usize,
    profile_scroll: gpui::ScrollHandle,
    custom_colors: Option<api::CustomColors>,
    device_colors: Option<Arc<DeviceColors>>,
    device_color_saving: bool,
    color_picker: Option<color_picker::Active>,
    accent_anchor: color_picker::Anchor,
    background_anchor: color_picker::Anchor,
    services: Option<Arc<dyn SettingsServices>>,
    auxiliary_error: Option<String>,
    remote: remote::RemoteView,
    ssh: ssh::SshView,
    focus: FocusHandle,
    close_focus: FocusHandle,
    focus_on_open: bool,
    access_review: std::rc::Rc<std::cell::Cell<bool>>,
    access_focus: FocusHandle,
    body_scroll: gpui::ScrollHandle,
    keyboard_focus: bool,
    controls: BTreeMap<String, FocusHandle>,
    tab_focus: Vec<FocusHandle>,
    tabs_scroll: gpui::ScrollHandle,
}
impl EventEmitter<SettingsEvent> for SettingsPanel {}
impl SettingsPanel {
    pub fn new(client: HostClient, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let forms = Forms::new(cx);
        let remote = remote::RemoteView::new(cx);
        let ssh = ssh::SshView::default();
        let services: Arc<dyn SettingsServices> =
            Arc::new(native_services::NativeServices::new(client.clone()));
        let panel = cx.new(|cx| Self {
            client,
            operations: operations::Operations::default(),
            semantic: None,
            semantic_actions: BTreeMap::new(),
            semantic_fields: BTreeMap::new(),
            draft: None,
            category: Category::General,
            palette: Palette::new("#ec4899", "#111016"),
            forms,
            subscriptions: vec![],
            loading: false,
            load_error: None,
            saved_visible: false,
            save_error: None,
            save_ready: false,
            visible: false,
            saved_epoch: 0,
            agent_error: None,
            environment: None,
            environment_subscription: None,
            restore_environment_focus: false,
            environment_write: None,
            environment_writing: false,
            catalog: None,
            catalog_error: None,
            profile_menu: false,
            profile_index: 0,
            profile_scroll: gpui::ScrollHandle::new(),
            custom_colors: None,
            device_colors: None,
            device_color_saving: false,
            color_picker: None,
            accent_anchor: Default::default(),
            background_anchor: Default::default(),
            services: Some(services),
            auxiliary_error: None,
            remote,
            ssh,
            focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            focus_on_open: false,
            access_review: Default::default(),
            access_focus: cx.focus_handle(),
            body_scroll: gpui::ScrollHandle::new(),
            keyboard_focus: false,
            controls: BTreeMap::new(),
            tab_focus: Category::ALL.iter().map(|_| cx.focus_handle()).collect(),
            tabs_scroll: gpui::ScrollHandle::new(),
        });
        panel.update(cx, |panel, cx| {
            for (key, field) in panel.forms.fields.clone() {
                panel.subscriptions.push(cx.observe_in(
                    &field,
                    window,
                    move |panel, field, _, cx| {
                        if field.read(cx).is_composing() {
                            cx.notify();
                            return;
                        }
                        let text = field.read(cx).text.clone();
                        panel.changed_input(key, text, cx);
                    },
                ));
            }
            for field in [panel.remote.hostname.clone(), panel.remote.auth.clone()] {
                panel
                    .subscriptions
                    .push(cx.observe_in(&field, window, |_, _, _, cx| cx.notify()));
            }
            let login_address = panel.remote.login_address.read(cx).edit_focus();
            panel
                .subscriptions
                .push(cx.on_focus(&login_address, window, |panel, _, cx| {
                    panel
                        .remote
                        .login_address
                        .update(cx, Field::select_all_text);
                }));
            panel.refresh(cx);
            panel.load_auxiliary(cx);
        });
        panel
    }
    pub fn set_palette(&mut self, palette: Palette, cx: &mut Context<Self>) {
        if self.palette == palette {
            return;
        }
        self.palette = palette;
        let mut fields = self.forms.fields.values().cloned().collect::<Vec<_>>();
        fields.extend([
            self.remote.hostname.clone(),
            self.remote.auth.clone(),
            self.remote.login_address.clone(),
        ]);
        if let Some(environment) = &self.environment {
            fields.push(environment.text.clone());
        }
        if let Some(editor) = &self.ssh.editor {
            fields.extend(editor.fields());
        }
        for field in fields {
            field.update(cx, |field, cx| field.apply_palette(palette, cx));
        }
        if let Some(picker) = &self.color_picker {
            picker.view.update(cx, |picker, cx| picker.set_palette(palette, cx));
        }
        cx.notify();
    }
}
