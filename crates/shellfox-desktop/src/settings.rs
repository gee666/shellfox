use crate::{field::Field, host_client::HostClient, theme::Palette};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Render, Subscription, Window, div,
    prelude::*, px,
};
use shellfox_core::{Settings, SettingsPatch};
use shellfox_service::native::{NativeAction, NativeResponse};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
#[path = "settings_agents.rs"]
mod agents;
#[path = "settings_api.rs"]
pub mod api;
#[path = "settings_aux.rs"]
mod aux;
#[path = "settings_catalog.rs"]
mod catalog_validation;
#[path = "settings_colors.rs"]
mod colors;
#[path = "settings_browser.rs"]
mod browser;
#[path = "settings_compat.rs"]
mod compat;
#[path = "settings_controller.rs"]
mod controller;
#[path = "settings_controls.rs"]
mod controls;
#[path = "settings_environment.rs"]
mod environment_data;
#[path = "settings_environment_keys.rs"]
mod environment_keys;
#[path = "settings_environment_rpc.rs"]
mod environment_rpc;
#[path = "settings_environment_view.rs"]
mod environment_view;
#[path = "settings_error.rs"]
mod errors;
#[path = "settings_device_colors.rs"]
mod device_colors;
pub use device_colors::DeviceColors;
#[path = "settings_device_color_actions.rs"]
mod device_color_actions;
#[path = "settings_field_semantics.rs"]
mod field_semantics;
#[path = "settings_forms.rs"]
mod forms;
#[path = "settings_general.rs"]
mod general;
#[path = "settings_key_picker.rs"]
mod key_picker;
#[path = "settings_keyboard.rs"]
mod keyboard;
#[path = "settings_layout.rs"]
mod layout;
#[path = "settings_login.rs"]
mod login;
#[path = "settings_native_gateway.rs"]
mod native_gateway;
#[path = "settings_native.rs"]
mod native_services;
#[path = "settings_native_ssh.rs"]
mod native_ssh;
#[cfg(test)]
#[path = "settings_native_ssh_tests.rs"]
mod native_ssh_tests;
#[path = "settings_native_state.rs"]
mod native_state;
#[cfg(test)]
#[path = "settings_native_tests.rs"]
mod native_tests;
#[path = "settings_operation_tracker.rs"]
mod operation_tracker;
#[path = "settings_operations.rs"]
mod operations;
#[path = "settings_profiles.rs"]
mod profile_keys;
#[path = "settings_progress.rs"]
mod progress;
#[path = "settings_remote.rs"]
mod remote;
#[path = "settings_remote_actions.rs"]
mod remote_actions;
#[path = "settings_remote_login_view.rs"]
mod remote_login_view;
#[path = "settings_remote_page.rs"]
mod remote_page;
#[path = "settings_remote_peers.rs"]
mod remote_peers;
#[path = "settings_remote_poll.rs"]
mod remote_poll;
#[path = "settings_rule_edits.rs"]
mod rule_edits;
#[path = "settings_rules.rs"]
mod rules;
#[path = "settings_semantic_control.rs"]
mod semantic_control;
#[path = "settings_semantic_element.rs"]
mod semantic_element;
#[path = "settings_semantic_enabled.rs"]
mod semantic_enabled;
#[cfg(test)]
#[path = "settings_semantic_tests.rs"]
mod semantic_tests;
#[path = "settings_semantics.rs"]
mod semantics;
#[path = "settings_ssh.rs"]
mod ssh;
#[path = "settings_ssh_form.rs"]
mod ssh_form;
#[path = "settings_ssh_intent.rs"]
mod ssh_intent;
#[path = "settings_ssh_open.rs"]
mod ssh_open;
pub use ssh_intent::SshOpenAdmission;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SshOpenRejection {
    Busy,
    Frozen,
}
#[path = "settings_ssh_page.rs"]
mod ssh_page;
#[path = "settings_state.rs"]
mod state;
#[path = "settings_terminals.rs"]
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
}
pub struct SettingsSshOpened {
    pub request_id: shellfox_core::SessionId,
    pub opened: shellfox_service::ssh::OpenedProfile,
    /// Clone, not fork: pending statuses/adoption belong to this exact controller.
    pub client: HostClient,
}
/// Failure stays on this panel's window-bound subscription and original intent.
/// There is no opening receipt to observe or adopt on this path.
pub struct SettingsSshOpenFailed {
    pub request_id: shellfox_core::SessionId,
    pub message: String,
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
    environment_write: Option<environment_rpc::PendingEnvironment>,
    environment_writing: bool,
    catalog: Option<api::Catalog>,
    catalog_error: Option<String>,
    profile_menu: bool,
    profile_index: usize,
    custom_colors: Option<api::CustomColors>,
    device_colors: Option<Arc<DeviceColors>>,
    device_color_saving: bool,
    services: Option<Arc<dyn SettingsServices>>,
    auxiliary_error: Option<String>,
    remote: remote::RemoteView,
    ssh: ssh::SshView,
    ssh_open_intent: ssh_intent::Intent,
    focus: FocusHandle,
    close_focus: FocusHandle,
    focus_on_open: bool,
    controls: BTreeMap<String, FocusHandle>,
    tab_focus: Vec<FocusHandle>,
}
impl EventEmitter<SettingsEvent> for SettingsPanel {}
impl EventEmitter<SettingsSshOpened> for SettingsPanel {}
impl EventEmitter<SettingsSshOpenFailed> for SettingsPanel {}
impl SettingsPanel {
    pub fn new(client: HostClient, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let forms = Forms::new(cx);
        let remote = remote::RemoteView::new(cx);
        let ssh = ssh::SshView::new(cx);
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
            environment_write: None,
            environment_writing: false,
            catalog: None,
            catalog_error: None,
            profile_menu: false,
            profile_index: 0,
            custom_colors: None,
            device_colors: None,
            device_color_saving: false,
            services: Some(services),
            auxiliary_error: None,
            remote,
            ssh,
            ssh_open_intent: ssh_intent::Intent::default(),
            focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            focus_on_open: false,
            controls: BTreeMap::new(),
            tab_focus: Category::ALL.iter().map(|_| cx.focus_handle()).collect(),
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
        cx.notify();
    }
}
