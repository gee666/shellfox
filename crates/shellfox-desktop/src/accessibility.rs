//! Desktop-only native accessibility. Semantics never participate in layout or styling.
//! Semantic views stay uncached. Each paint collects current measured nodes; the outer
//! finish boundary publishes only after deferred overlays have painted. Child entities
//! retain the window's shared collector and exact-key ID registry, not old node snapshots.
//! See docs/gpui-accessibility.md for lifecycle and platform acceptance requirements.
#[path = "accessibility/bridge.rs"]
mod bridge;
#[path = "accessibility/context.rs"]
mod context;
#[path = "accessibility/element.rs"]
mod element;
#[path = "accessibility/frame.rs"]
mod frame;
#[path = "accessibility/ids.rs"]
mod ids;
#[path = "accessibility/native.rs"]
mod native;
#[path = "accessibility/overlay.rs"]
mod overlay;
#[path = "accessibility/scope.rs"]
mod scope;
#[path = "accessibility/terminal.rs"]
mod terminal;
#[path = "accessibility/tree.rs"]
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
#[path = "accessibility/tests.rs"]
mod tests;
