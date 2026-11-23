//! The terminals of the selected local session: their views, the tab strip, ordering,
//! focus and menus.
pub(crate) mod activity;
pub(crate) mod close;
pub(crate) mod focus;
pub(crate) mod menu;
pub(crate) mod pane;
pub(crate) mod profile_menu;
pub(crate) mod reorder;
pub(crate) mod scroll;
pub(crate) mod strip;
pub(crate) use menu::TabMenu;
pub(crate) use scroll::TabScroll;

use crate::{root::StartupError, terminal::TerminalView};
use gpui::Entity;
use shellfox_host::TabId;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(crate) struct TabsState {
    /// Attached terminal views; a tab without one is attaching or failed.
    pub(crate) views: HashMap<TabId, Entity<TerminalView>>,
    pub(crate) attaching: HashSet<TabId>,
    pub(crate) failures: HashMap<TabId, StartupError>,
    pub(crate) scroll: TabScroll,
    pub(crate) menu: Option<TabMenu>,
    pub(crate) activity: activity::TabActivity,
    pub(crate) profile_menu: Option<profile_menu::ProfileMenu>,
}
