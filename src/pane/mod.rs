//! Pane windowing system: a draggable, resizable, card-stacking pane manager.
//!
//! ## Architecture
//!
//! This module was extracted from a monolithic ~1770-line `paned.rs` during
//! refactoring to improve modularity and learnability. It's organized into:
//!
//! - [`model`] — data types (`PaneState`, `DragInfo`, etc.) and layout
//!   constants. Pure data, no Floem dependencies.
//! - [`layout`] — card-stack positioning algorithms that compute `target_x`
//!   for each pane based on dock order, window width, and focus.
//! - [`animation`] — frame-scheduled easing loop that smoothly moves panes
//!   from their current `x` to their `target_x`.
//! - [`views`] — Floem UI components (toolbar, pane cards, browser content,
//!   resize handles).
//!
//! ## The `PaneCtx` pattern
//!
//! The original code passed 11+ individual `RwSignal` parameters to every
//! function, leading to unwieldy signatures. `PaneCtx` bundles them into a
//! single struct that is `Clone + Copy` (because `RwSignal` is `Copy`).
//!
//! This is a common Floem pattern when many signals need to travel together:
//! group them in a `#[derive(Clone, Copy)]` struct and pass it by value.
//! It's not a "context provider" like React — it's just a convenience struct
//! that's cheap to pass around.

pub mod animation;
pub mod layout;
pub mod model;
pub mod views;

use std::collections::HashMap;

use floem::prelude::*;

use model::*;

/// Bundles the reactive signals that drive the pane windowing system.
///
/// Passed by copy to view functions and animation helpers, replacing
/// the 11+ positional signal parameters that were previously threaded
/// through every function call.
///
/// ## Why `#[derive(Clone, Copy)]`?
///
/// `RwSignal<T>` is `Copy` regardless of `T` — it's a lightweight ID
/// (just a `usize` internally) pointing into Floem's global signal store.
/// So a struct of `RwSignal`s is also `Copy`, making it free to pass
/// into `move` closures without worrying about ownership.
#[derive(Clone, Copy)]
pub struct PaneCtx {
    /// The list of all open panes with their layout state.
    pub panes: RwSignal<Vec<PaneState>>,
    /// Monotonically increasing counter for unique pane IDs.
    pub next_pane_id: RwSignal<usize>,
    /// Active drag operation, if any. `None` when no drag is in progress.
    pub dragging: RwSignal<Option<DragInfo>>,
    /// Active resize operation, if any.
    pub resizing: RwSignal<Option<ResizeInfo>>,
    /// Current window dimensions `(width, height)`.
    pub window_size: RwSignal<(f64, f64)>,
    /// True while the animation loop is running. Prevents multiple
    /// concurrent animation loops from fighting over pane positions.
    pub animating: RwSignal<bool>,
    /// ID of the currently focused pane, or `None`. Determines which
    /// pane stays fully visible in the card-stack when overflow occurs.
    pub focus_pane_id: RwSignal<Option<usize>>,
    /// Lightweight counter bumped each animation frame so style closures
    /// re-evaluate without dyn_stack re-diffing the whole pane list.
    ///
    /// ## Why not just read `panes` directly?
    ///
    /// Reading `panes.get()` in a style closure would clone the entire
    /// `Vec<PaneState>` every frame. Instead, style closures read
    /// `anim_tick.get()` (a cheap u64 copy) to subscribe, then use
    /// `panes.with_untracked(|p| ...)` to borrow without cloning.
    pub anim_tick: RwSignal<u64>,
    /// Bumped when panes are added or removed (structural changes).
    /// `dyn_stack` reads this to know when to re-diff the pane list.
    /// Separated from `anim_tick` so that animation frames (which only
    /// change positions) don't trigger expensive list diffing.
    pub pane_version: RwSignal<u64>,
    /// Maps `channel_id` → a focus trigger signal. When a channel's pane
    /// is clicked or navigated to, its trigger is bumped, causing the
    /// text input in that pane to request focus.
    pub focus_triggers: RwSignal<HashMap<usize, RwSignal<u64>>>,
    /// When true, the window is transparent with per-region click-through.
    /// Mouse events only reach panes and the toolbar; clicks on the
    /// transparent background pass through to the desktop.
    pub pseudo_window: RwSignal<bool>,
}

impl PaneCtx {
    /// Kick off the animation loop (delegates to ``animation::start_animation``).
    pub fn start_animation(self) {
        animation::start_animation(self);
    }
}
