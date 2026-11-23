//! Desktop-only native accessibility. Semantics never participate in layout or styling.
//! Semantic views stay uncached. Each paint collects current measured nodes; the outer
//! finish boundary publishes only after deferred overlays have painted. Child entities
//! retain the window's shared collector and exact-key ID registry, not old node snapshots.
//! See docs/gpui-accessibility.md for lifecycle and platform acceptance requirements.
mod bridge;
mod context;
mod element;
mod frame;
mod ids;
mod native;
mod overlay;
mod scope;
mod terminal;
mod tree;

pub use accesskit::{Action, Node, NodeId as SemanticId, Role, TextSelection};
pub use bridge::{SemanticAction, SemanticActionKind};
pub use context::WindowSemanticContext;
pub use element::SemanticElement;
pub use frame::{SemanticFrame, SemanticNode};
pub use native::{HiddenWindowPresentation, NativeAccessibility, WindowPresentation};
pub use terminal::{TerminalDocument, TerminalGeometry, TerminalSnapshot, TerminalViewStamp};
pub use tree::{SemanticError, SemanticTree};

#[cfg(test)]
use terminal::{TerminalPoint, TerminalSelection};

#[cfg(test)]
mod tests;
