//! The tab strip of the selected local session: rendering, ordering, focus and menus.
pub(crate) mod strip;
pub(crate) mod close;
pub(crate) mod reorder;
pub(crate) mod scroll;
pub(crate) mod focus;
pub(crate) mod menu;
pub(crate) mod activity;
pub(crate) mod profile_menu;
pub(crate) mod pane;
pub(crate) use menu::TabMenu;
pub(crate) use scroll::TabScroll;
