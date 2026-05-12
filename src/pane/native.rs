//! Native multi-window mode: each pane as a real OS window.
//!
//! This module manages the lifecycle of per-pane OS windows in the native
//! multi-window mode (`native_paned` binary). It translates the layout
//! algorithm's window-relative coordinates into screen-absolute positions
//! and applies them via `set_window_inner_bounds`.
//!
//! ## Coordinate model
//!
//! The layout algorithm in [`super::layout`] computes positions in a virtual
//! coordinate space. This module offsets those by `anchor_origin` (the
//! screen-space top-left of the monitor the management window is on) to
//! produce absolute screen coordinates for each pane window.
//!
//! ## Window sync
//!
//! [`sync_windows`] is the main entry point. It diffs the current `panes`
//! signal against the `window_ids` map and:
//! - Spawns new OS windows for panes that don't have one yet.
//! - Closes OS windows for panes that were removed.
//! - Repositions existing windows to match the layout.
//!
//! All pane windows are undecorated and always-on-top.

use std::collections::HashMap;

use floem::kurbo::{Point, Rect};
use floem::prelude::*;
use floem::window::{WindowConfig, WindowIdExt, WindowLevel};

use super::PaneCtx;
use super::model::*;
use crate::data::{Channel, Message, Server};

use super::views::pane_content;

/// Translate a pane's current animated position from virtual coords
/// to an absolute screen `Rect`.
fn pane_screen_rect(pane: &PaneState, anchor: (f64, f64), virtual_height: f64) -> Rect {
    let x = anchor.0 + pane.render_x();
    let y = anchor.1 + pane.render_top(virtual_height);
    let w = pane.render_width();
    let h = pane.display_height();
    Rect::new(x, y, x + w, y + h)
}

/// Reposition all existing pane windows to match the current layout state.
/// Called each animation frame in native mode.
///
/// The pane currently being dragged is **skipped** — its window is
/// repositioned directly by the PointerMove handler in `pane_content`
/// to avoid coordinate-frame conflicts (the animation loop and the
/// drag handler would fight over the window position, causing the
/// local pointer coords to drift).
pub fn reposition_all_windows(ctx: PaneCtx) {
    let anchor = ctx.anchor_origin.get_untracked();
    let (_, vh) = ctx.window_size.get_untracked();
    let window_ids = ctx.window_ids.get_untracked();
    let drag_id = ctx.dragging.get_untracked()
        .filter(|d| d.moved)
        .map(|d| d.pane_id);

    ctx.panes.with_untracked(|panes| {
        for pane in panes {
            if Some(pane.id) == drag_id {
                continue;
            }
            if let Some(wid) = window_ids.get(&pane.id) {
                let rect = pane_screen_rect(pane, anchor, vh);
                wid.set_window_inner_bounds(rect);
            }
        }
    });
}

/// Reposition a single pane window immediately. Used during drag to move
/// the dragged pane's window from the PointerMove handler without waiting
/// for the animation loop.
///
/// Unlike `reposition_all_windows`, this uses the pane's raw `y` position
/// instead of `render_top()`. During drag, `pane.y` tracks the pointer
/// even while docked, but `render_top()` clamps to `dock_y`. Using the
/// raw value gives the user visual feedback during the undock attempt.
pub fn reposition_single_window(ctx: PaneCtx, pane_id: usize) {
    let anchor = ctx.anchor_origin.get_untracked();
    if let Some(wid) = ctx.window_ids.with_untracked(|m| m.get(&pane_id).copied()) {
        ctx.panes.with_untracked(|panes| {
            if let Some(pane) = panes.iter().find(|p| p.id == pane_id) {
                let x = anchor.0 + pane.render_x();
                let y = anchor.1 + pane.y;
                let w = pane.render_width();
                let h = pane.display_height();
                wid.set_window_inner_bounds(Rect::new(x, y, x + w, y + h));
            }
        });
    }
}

/// Diff `panes` vs `window_ids` and spawn/close OS windows as needed.
/// Also repositions all surviving windows.
pub fn sync_windows(
    ctx: PaneCtx,
    servers: RwSignal<Vec<Server>>,
    channels: RwSignal<Vec<Channel>>,
    messages: RwSignal<HashMap<usize, Vec<Message>>>,
    next_message_id: RwSignal<usize>,
    active_server: RwSignal<usize>,
) {
    let anchor = ctx.anchor_origin.get_untracked();
    let (_, vh) = ctx.window_size.get_untracked();

    // Snapshot current pane IDs.
    let current_pane_ids: Vec<(usize, PaneKind)> = ctx.panes.with_untracked(|p| {
        p.iter().map(|ps| (ps.id, ps.kind.clone())).collect()
    });
    let current_ids: std::collections::HashSet<usize> =
        current_pane_ids.iter().map(|(id, _)| *id).collect();

    let old_ids: std::collections::HashSet<usize> =
        ctx.window_ids.with_untracked(|m| m.keys().copied().collect());

    // Close windows for removed panes.
    for &removed_id in old_ids.difference(&current_ids) {
        if let Some(wid) = ctx.window_ids.with_untracked(|m| m.get(&removed_id).copied()) {
            floem::window::close_window(wid);
        }
        ctx.window_ids.update(|m| { m.remove(&removed_id); });
    }

    // Snap new panes' x to target_x so OS windows appear at the
    // correct dock slot directly. In single-window mode the pane
    // slides in from off-screen, but in native mode an off-screen
    // window is invisible.
    let new_pane_ids: Vec<usize> = current_ids.difference(&old_ids).copied().collect();
    if !new_pane_ids.is_empty() {
        ctx.panes.update(|p| {
            for pane in p.iter_mut() {
                if new_pane_ids.contains(&pane.id) {
                    pane.x = pane.target_x;
                }
            }
        });
    }

    // Spawn windows for newly added panes.
    for (pane_id, kind) in &current_pane_ids {
        if old_ids.contains(pane_id) {
            continue;
        }
        let pid = *pane_id;
        let k = kind.clone();

        // Compute initial position from current (snapped) layout state.
        let rect = ctx.panes.with_untracked(|p| {
            p.iter()
                .find(|ps| ps.id == pid)
                .map(|ps| pane_screen_rect(ps, anchor, vh))
                .unwrap_or(Rect::new(0.0, 0.0, DEFAULT_PANE_WIDTH, DEFAULT_PANE_HEIGHT))
        });

        let config = WindowConfig::default()
            .position(Point::new(rect.x0, rect.y0))
            .size((rect.width(), rect.height()))
            .undecorated(true)
            .with_transparent(true)
            .window_level(WindowLevel::AlwaysOnTop);

        floem::window::new_window(
            move |window_id| {
                ctx.window_ids.update(|m| { m.insert(pid, window_id); });

                pane_content(
                    pid, k, servers, channels, messages,
                    next_message_id, active_server, ctx,
                )
            },
            Some(config),
        );
    }

    // Reposition surviving windows.
    reposition_all_windows(ctx);

    // After closing a pane's OS window, the OS may activate an arbitrary
    // window (often the browser). Explicitly focus the pane that should
    // have focus so the correct window comes to the front.
    if old_ids.difference(&current_ids).next().is_some() {
        if let Some(fid) = ctx.focus_pane_id.get_untracked() {
            if let Some(wid) = ctx.window_ids.with_untracked(|m| m.get(&fid).copied()) {
                use floem::WindowIdExt;
                wid.focus_window();
            }
        }
    }
}
