//! Pane data model types and layout constants for the paned window manager.
//!
//! ## Design notes
//!
//! This module contains only data structures and constants — no Floem
//! imports, no UI code. This separation keeps the model testable and
//! understandable independently of the UI framework.
//!
//! ### `PaneState` — the central data structure
//!
//! Each open pane has a `PaneState` with two key position fields:
//! - `x` — the *current* rendered position (updated by animation).
//! - `target_x` — the *desired* position computed by the layout algorithm.
//!
//! The animation system (in `animation.rs`) smoothly moves `x` toward
//! `target_x` each frame. This two-field pattern is common in UI toolkits
//! that want smooth transitions without CSS-style transition declarations.
//!
//! ### Card-stack overflow
//!
//! When docked panes don't all fit at full width, the layout algorithm
//! (in `layout.rs`) compresses overflow panes into "peek strips" — narrow
//! tabs at the left and right edges, like a fanned deck of cards. The
//! `stack_side` and `z_order` fields on `PaneState` control this behavior.
//!
//! ### `dock_order` — stable ordering independent of Vec position
//!
//! Panes are stored in a `Vec`, but their left-to-right visual order is
//! determined by `dock_order`, not by their position in the Vec. Lower
//! `dock_order` = further right in the window. This decoupling lets us
//! reorder panes during drag without moving them in the Vec (which would
//! invalidate indices).

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

/// Default width for newly created chat panes.
pub const DEFAULT_PANE_WIDTH: f64 = 350.0;
/// Width for the server/channel browser pane (narrower than chat panes).
pub const BROWSER_PANE_WIDTH: f64 = 240.0;
/// Default height for newly created panes.
pub const DEFAULT_PANE_HEIGHT: f64 = 500.0;
/// Horizontal gap between adjacent docked panes.
pub const PANE_SPACING: f64 = 8.0;
/// Height of the toolbar strip at the top of the window.
pub const TOOLBAR_HEIGHT: f64 = 48.0;
/// Height of each pane's header (drag handle + close button area).
pub const PANE_HEADER_HEIGHT: f64 = 36.0;
/// Vertical distance a docked pane must be dragged upward to undock it.
pub const UNDOCK_THRESHOLD: f64 = 40.0;
/// Minimum total pointer displacement before a click becomes a drag.
/// Prevents accidental drags when the user just wants to click.
pub const DRAG_DEAD_ZONE: f64 = 5.0;
/// Default window height (used for initial layout calculations).
pub const WINDOW_HEIGHT: f64 = 700.0;
/// Width of the invisible side resize handles.
pub const RESIZE_HANDLE_WIDTH: f64 = 4.0;
/// Minimum width a pane can be resized to.
pub const MIN_PANE_WIDTH: f64 = 120.0;
/// Minimum height a pane can be resized to.
pub const MIN_PANE_HEIGHT: f64 = 100.0;
/// Collapsed (header-only) panes can be resized narrower than normal.
pub const COLLAPSED_MIN_WIDTH: f64 = 60.0;
/// Width a pane shrinks to when collapsed (header-only). Wide enough
/// to show the icon, a truncated channel name, and the close button.
pub const COLLAPSED_PANE_WIDTH: f64 = 140.0;
/// Corner resize handles are slightly larger for easier targeting.
pub const CORNER_HANDLE_SIZE: f64 = 8.0;
/// How much of a stacked pane's tab strip is visible in the card-stack.
/// This is the width of the "peek" tab when a pane is compressed.
pub const PEEK_WIDTH: f64 = 28.0;
/// Minimum px/frame the animation moves (prevents crawling to a halt
/// when the remaining distance is very small but ANIM_FACTOR produces
/// a tiny step).
pub const MIN_ANIM_SPEED: f64 = 12.0;
/// Proportion of remaining distance moved each frame (ease-out).
/// 0.30 means "move 30% of the remaining distance per frame", creating
/// a decelerating animation that starts fast and slows down.
pub const ANIM_FACTOR: f64 = 0.30;
/// Below this distance, snap exactly to target (prevents sub-pixel jitter).
pub const ANIM_SNAP: f64 = 1.0;
/// If true, new panes open to the left of existing panes; otherwise to the right.
pub const OPEN_PANES_LEFT: bool = true;

// ---------------------------------------------------------------------------
// Pane types
// ---------------------------------------------------------------------------

/// What kind of content a pane displays.
#[derive(Clone, Debug)]
pub enum PaneKind {
    /// Server/channel browser pane — shows the server icon strip and
    /// channel list. Only one browser pane should exist at a time.
    Browser,
    /// Chat timeline for a specific channel, identified by `channel_id`.
    Chat { channel_id: usize },
}

impl PaneKind {
    /// Returns `Some(channel_id)` for chat panes, `None` for browser.
    pub fn channel_id(&self) -> Option<usize> {
        match self {
            PaneKind::Chat { channel_id } => Some(*channel_id),
            PaneKind::Browser => None,
        }
    }
}

/// Identifies an open pane with its layout state.
///
/// ## Position model
///
/// - `target_x` is the "slot" position computed by the layout algorithm
///   (right-aligned, packed with spacing).
/// - `x` is the current rendered position, animated toward `target_x`
///   each frame by the animation system.
/// - During a drag, `x` tracks the mouse directly while other panes'
///   `target_x` values are recomputed to make room.
///
/// ## Docked vs floating
///
/// - `docked == true`: pane is anchored to the bottom of the window.
///   Its top edge is at `window_height - height`. Horizontal position
///   is computed by the layout algorithm.
/// - `docked == false`: pane floats freely. Both `x` and `y` are
///   user-controlled via drag. The pane re-docks when dragged near the
///   bottom edge.
#[derive(Clone, Debug)]
pub struct PaneState {
    pub id: usize,
    pub kind: PaneKind,
    /// Current rendered horizontal position (animated toward `target_x`).
    pub x: f64,
    /// Desired horizontal slot position; other panes animate toward this.
    pub target_x: f64,
    pub width: f64,
    pub height: f64,
    pub docked: bool,
    /// Only meaningful when `docked == false`: top edge in window coords.
    pub y: f64,
    /// When true, only the header bar is visible (body is hidden).
    /// Toggled by clicking a focused pane's header.
    pub collapsed: bool,
    /// Stores the pre-collapse width so it can be restored when uncollapsing.
    /// 0.0 when the pane is not collapsed (i.e. unused).
    pub uncollapsed_width: f64,
    /// Stable insertion order; lower = further right, higher = further left.
    /// New pane placement depends on ``OPEN_PANES_LEFT``.
    pub dock_order: usize,
    /// Which edge this pane is compressed into, or `None` if fully visible.
    /// Set by the layout algorithm when panes overflow the window width.
    pub stack_side: Option<StackSide>,
    /// Computed z-index for rendering order.
    /// - 0: default (everything fits, no stacking)
    /// - 100+: stacked panes (higher = closer to visible area)
    /// - 200: fully visible panes
    /// - 300: currently being dragged (always on top)
    pub z_order: i32,
    /// Pixels of width collapsed away during the stacking animation.
    /// 0.0 = fully expanded, ``(width - PEEK_WIDTH)`` = fully collapsed.
    /// Animated by ``tick_animation`` toward a target derived from ``stack_side``.
    pub collapse_width: f64,
    /// Remembers which side the collapse animation is for.
    /// Set when stacking begins (copied from ``stack_side``), persists through
    /// the unstacking expansion so ``render_x``/``render_width`` know which
    /// direction to animate, then cleared when ``collapse_width`` reaches 0.
    pub collapse_side: Option<StackSide>,
}

/// `PaneState` identity is based solely on `id` — two panes with the same
/// id are considered the same pane regardless of position or other fields.
/// This is required by `dyn_stack` for its key-based diffing.
impl PartialEq for PaneState {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for PaneState {}

impl std::hash::Hash for PaneState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers — encapsulate state-dependent sizing
// ---------------------------------------------------------------------------

impl PaneState {
    /// Visual height: header-only when collapsed, full content height otherwise.
    pub fn display_height(&self) -> f64 {
        if self.collapsed {
            PANE_HEADER_HEIGHT
        } else {
            self.height
        }
    }

    /// Y position where this pane's top edge sits when docked to the bottom.
    pub fn dock_y(&self, window_height: f64) -> f64 {
        window_height - self.display_height()
    }

    /// Top edge for rendering: anchored to bottom when docked, free when floating.
    pub fn render_top(&self, window_height: f64) -> f64 {
        if self.docked {
            self.dock_y(window_height)
        } else {
            self.y
        }
    }

    /// Rendered width, smoothly interpolated during collapse/expand animation.
    /// During animation ``collapse_width`` moves from 0 toward
    /// ``(width - PEEK_WIDTH)``; the rendered width is the difference.
    pub fn render_width(&self) -> f64 {
        if self.collapse_side.is_some() {
            self.width - self.collapse_width
        } else {
            self.width
        }
    }

    /// Rendered x position, smoothly interpolated during collapse/expand.
    /// For right-stacked panes the left edge moves rightward by
    /// ``collapse_width`` so the visible strip stays anchored to the right.
    pub fn render_x(&self) -> f64 {
        match self.collapse_side {
            Some(StackSide::Right) => self.x + self.collapse_width,
            _ => self.x,
        }
    }

    /// True when the collapse animation has fully completed (pane is a peek tab).
    pub fn is_fully_collapsed(&self) -> bool {
        self.collapse_side.is_some()
            && (self.width - PEEK_WIDTH - self.collapse_width).abs() < ANIM_SNAP
    }

    /// True when a collapse or expand animation is in progress.
    pub fn is_collapse_animating(&self) -> bool {
        self.collapse_side.is_some() && self.collapse_width > 0.0
    }

    /// Minimum width allowed during resize (collapsed panes can go narrower).
    pub fn min_resize_width(&self) -> f64 {
        if self.collapsed {
            COLLAPSED_MIN_WIDTH
        } else {
            MIN_PANE_WIDTH
        }
    }
}

/// Find the best pane to focus after closing the pane with `closing_id`.
///
/// Prefers the left neighbor (next higher ``dock_order``), falling back to
/// the right neighbor (next lower ``dock_order``). Returns ``None`` if no
/// other panes remain.
pub fn neighbor_focus(panes: &[PaneState], closing_id: usize) -> Option<usize> {
    let closing_order = panes.iter().find(|p| p.id == closing_id)?.dock_order;
    // Exclude the browser pane — it's pinned to the right edge and shouldn't
    // receive focus when closing chat panes.
    let others = panes
        .iter()
        .filter(|p| p.id != closing_id && !matches!(p.kind, PaneKind::Browser));

    // Left neighbor: smallest dock_order greater than the closing pane's.
    let left = others
        .clone()
        .filter(|p| p.dock_order > closing_order)
        .min_by_key(|p| p.dock_order);

    // Right neighbor: largest dock_order less than the closing pane's.
    let right = others
        .filter(|p| p.dock_order < closing_order)
        .max_by_key(|p| p.dock_order);

    left.or(right).map(|p| p.id)
}

// ---------------------------------------------------------------------------
// Drag state
// ---------------------------------------------------------------------------

/// Tracks an in-progress drag operation on a pane header.
///
/// ## Click vs drag distinction
///
/// The `moved` flag starts `false`. The pointer-move handler accumulates
/// displacement and only sets `moved = true` once the total exceeds
/// `DRAG_DEAD_ZONE`. If the pointer is released with `moved == false`,
/// it's treated as a click (toggle collapse or scroll stacked pane into view).
#[derive(Clone, Copy, Debug)]
pub struct DragInfo {
    pub pane_id: usize,
    /// Pointer position when the drag started (for dead zone calculation).
    pub start_pointer_x: f64,
    pub start_pointer_y: f64,
    /// Previous frame's pointer position (for computing deltas).
    pub last_pointer_x: Option<f64>,
    pub last_pointer_y: Option<f64>,
    /// True once the pointer has moved past DRAG_DEAD_ZONE (distinguishes click from drag).
    pub moved: bool,
    /// Tracks the last committed insert position for hysteresis during reorder.
    /// Prevents jittery back-and-forth reordering when the drag position is
    /// near a boundary between two panes.
    pub last_insert_pos: Option<usize>,
    /// Whether the pane was already focused before this interaction started.
    /// If false, the first click just focuses (and focuses text input);
    /// only a second click toggles collapse.
    pub was_focused: bool,
}

// ---------------------------------------------------------------------------
// Stack side & resize types
// ---------------------------------------------------------------------------

/// Which edge a pane is compressed into during card-stack overflow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StackSide {
    /// Compressed into the left edge of the window.
    Left,
    /// Compressed into the right edge of the window.
    Right,
}

/// Which edge of a pane is being resized.
#[derive(Clone, Copy, Debug)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    TopLeft,
    TopRight,
}

/// Tracks an in-progress resize operation.
#[derive(Clone, Copy, Debug)]
pub struct ResizeInfo {
    pub pane_id: usize,
    pub edge: ResizeEdge,
    /// Previous frame's pointer position for computing deltas.
    pub last_x: Option<f64>,
    pub last_y: Option<f64>,
}
