//! Paned demo binary — floating pane window manager with drag, resize,
//! card-stack overflow, and transparent click-through.
//!
//! ## How this differs from `unified.rs`
//!
//! - **Undecorated + transparent** — no OS title bar; uses a custom toolbar
//!   with drag grip, pin, and pseudo-window controls.
//! - **Multiple panes** — each channel gets its own pane card that can be
//!   dragged, resized, docked/undocked, and collapsed.
//! - **Card-stack overflow** — when panes don't all fit, excess panes
//!   compress into "peek strips" at the window edges.
//! - **Pseudo-window mode** — the window background is transparent and
//!   clicks on empty areas pass through to the desktop.
//!
//! ## Event handling architecture
//!
//! Pointer events (move, up) are handled at the root level rather than on
//! individual panes. This is because:
//! 1. Drag/resize operations span multiple panes.
//! 2. The mouse can leave the originating pane during a drag.
//! 3. PointerUp needs to finalize state regardless of where the cursor is.
//!
//! The root view's event handlers check `ctx.dragging` and `ctx.resizing`
//! to determine which operation is active, and dispatch accordingly.
//!
//! ## `dyn_stack` for the pane list
//!
//! Unlike the channel/server lists which use `VirtualStack`, the pane list
//! uses `dyn_stack` because:
//! - Panes are absolutely positioned (not in a scrollable column).
//! - There are typically few panes (< 20).
//! - Each pane has complex, unique content.
//!
//! The data closure reads `pane_version` (bumped on add/remove) and uses
//! `panes.get_untracked()` for the actual data. This means dyn_stack only
//! re-diffs on structural changes, not on every animation frame.

use std::collections::HashMap;

use floem::prelude::*;
use floem::views::{Decorators, dyn_stack};
use floem::window::WindowConfig;

use floem_demo::data::AppState;
use floem_demo::pane::PaneCtx;
use floem_demo::pane::layout::{
    commit_drag_order, recompute_dock_targets, recompute_targets_during_drag, update_input_regions,
};
use floem_demo::pane::model::*;
use floem_demo::pane::views::{pane_card, toolbar};
use floem_demo::theme;

/// Build the root view: toolbar + absolutely-positioned pane cards.
///
/// This function:
/// 1. Creates `AppState` for domain data (servers, channels, messages).
/// 2. Creates `PaneCtx` for pane management signals.
/// 3. Builds the toolbar and pane area.
/// 4. Attaches root-level event handlers for resize, drag, and keyboard.
fn app_view() -> impl IntoView {
    let state = AppState::with_sample_data();

    // Destructure signals from AppState. These are passed to pane_card
    // for building chat content inside each pane.
    let servers = state.servers;
    let channels = state.channels;
    let messages = state.messages;
    let active_server = state.active_server;
    let next_message_id = state.next_message_id;

    let initial_width = 1200.0;

    // Start with the browser pane docked to the right edge.
    let initial_x = initial_width - BROWSER_PANE_WIDTH - PANE_SPACING;

    // Create the PaneCtx — the bundle of reactive signals that control
    // the pane windowing system. This replaces the 11+ individual
    // signal parameters that were originally passed to every function.
    let ctx = PaneCtx {
        panes: RwSignal::new(vec![PaneState {
            id: 0,
            kind: PaneKind::Browser,
            x: initial_x,
            target_x: initial_x,
            width: BROWSER_PANE_WIDTH,
            height: DEFAULT_PANE_HEIGHT,
            docked: true,
            y: WINDOW_HEIGHT - DEFAULT_PANE_HEIGHT,
            collapsed: false,
            uncollapsed_width: 0.0,
            dock_order: 0,
            stack_side: None,
            z_order: 0,
        }]),
        next_pane_id: RwSignal::new(1usize),
        dragging: RwSignal::new(None),
        resizing: RwSignal::new(None),
        window_size: RwSignal::new((initial_width, WINDOW_HEIGHT)),
        animating: RwSignal::new(false),
        focus_pane_id: RwSignal::new(Some(0)),
        anim_tick: RwSignal::new(0),
        pane_version: RwSignal::new(0),
        focus_triggers: RwSignal::new(HashMap::new()),
        pseudo_window: RwSignal::new(false),
    };

    let toolbar = toolbar(ctx);

    // `dyn_stack` creates a view for each pane, diffing by pane ID.
    // The data closure reads `pane_version` to subscribe only to
    // structural changes (add/remove), then uses `get_untracked()` for
    // the actual data to avoid subscribing to animation updates.
    let pane_area = dyn_stack(
        move || {
            ctx.pane_version.get();
            ctx.panes.get_untracked()
        },
        |ps: &PaneState| ps.id,
        move |ps: PaneState| {
            pane_card(
                ps.id,
                ps.kind.clone(),
                servers,
                channels,
                messages,
                next_message_id,
                active_server,
                ctx,
            )
        },
    )
    .style(|s| s.width_full().height_full());

    // Push initial input regions only if starting in pseudo-window mode.
    if ctx.pseudo_window.get_untracked() {
        let (ww, wh) = (initial_width, WINDOW_HEIGHT);
        ctx.panes.with_untracked(|p| update_input_regions(p, ww, wh));
    }

    // --- Root view: pane area + toolbar overlay ---
    // Using `Stack::new` (not horizontal/vertical) because toolbar is
    // absolutely positioned and doesn't participate in flex layout.
    Stack::new((pane_area, toolbar))
        .style(move |s| {
            // Reactive background: transparent in pseudo-window mode,
            // solid dark otherwise.
            let bg = if ctx.pseudo_window.get() {
                Color::TRANSPARENT
            } else {
                theme::CHAT_BG
            };
            s.width_full().height_full().background(bg)
        })
        // --- Window resize handler ---
        // `WindowResized` fires when the OS window is resized.
        // We recompute pane positions for the new width and refresh
        // input regions if in pseudo-window mode.
        .on_event_cont(
            listener::WindowResized,
            move |_, size: &floem::kurbo::Size| {
                let old = ctx.window_size.get_untracked();
                ctx.window_size.set((size.width, size.height));
                // Only re-layout if width changed significantly (avoids
                // unnecessary work for tiny sub-pixel changes).
                if (old.0 - size.width).abs() > 1.0 {
                    let fid = ctx.focus_pane_id.get_untracked();
                    ctx.panes.update(|p| recompute_dock_targets(p, size.width, fid));
                    ctx.start_animation();
                }
                if ctx.pseudo_window.get_untracked() {
                    ctx.panes.with_untracked(|p| {
                        update_input_regions(p, size.width, size.height);
                    });
                }
            },
        )
        // --- Pointer move handler ---
        // Handles both resize and drag operations. Priority: resize > drag.
        // Each operation reads the current state from its signal, applies
        // deltas, and writes back the updated state.
        .on_event_cont(listener::PointerMove, move |_, event| {
            let pos = event.current.logical_point();

            // --- Resize ---
            if let Some(mut rz) = ctx.resizing.get_untracked() {
                let has_prev = rz.last_x.is_some() || rz.last_y.is_some();
                if has_prev {
                    // Compute pointer delta from last frame.
                    let dx = rz.last_x.map(|lx| pos.x - lx).unwrap_or(0.0);
                    let dy = rz.last_y.map(|ly| pos.y - ly).unwrap_or(0.0);
                    let (ww, _) = ctx.window_size.get_untracked();
                    ctx.panes.update(|p| {
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == rz.pane_id) {
                            let min_w = pane.min_resize_width();
                            // Apply horizontal resize based on which edge is being dragged.
                            match rz.edge {
                                ResizeEdge::Right | ResizeEdge::TopRight => {
                                    pane.width = (pane.width + dx).max(min_w);
                                }
                                ResizeEdge::Left | ResizeEdge::TopLeft => {
                                    // Left-edge resize: shrink width and shift x rightward.
                                    let new_w = (pane.width - dx).max(min_w);
                                    let delta = pane.width - new_w;
                                    pane.x += delta;
                                    pane.width = new_w;
                                }
                                ResizeEdge::Top => {}
                            }
                            // Apply vertical resize: dragging the top edge up
                            // increases height (and shifts y for undocked panes).
                            match rz.edge {
                                ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight => {
                                    let new_h = (pane.height - dy).max(MIN_PANE_HEIGHT);
                                    if !pane.docked {
                                        let actual_dy = pane.height - new_h;
                                        pane.y += actual_dy;
                                    }
                                    pane.height = new_h;
                                }
                                _ => {}
                            }
                        }
                        // Re-pack other panes around the resized one.
                        let mut no_hysteresis = None;
                        recompute_targets_during_drag(p, rz.pane_id, ww, &mut no_hysteresis);
                        // Keep the resized pane in place (don't let layout move it).
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == rz.pane_id) {
                            pane.target_x = pane.x;
                        }
                    });
                    ctx.anim_tick.set(ctx.anim_tick.get_untracked() + 1);
                    ctx.start_animation();
                }
                // Record current position for next frame's delta calculation.
                rz.last_x = Some(pos.x);
                rz.last_y = Some(pos.y);
                ctx.resizing.set(Some(rz));
                return;
            }

            // --- Drag ---
            if let Some(mut drag) = ctx.dragging.get_untracked() {
                // Stacked panes can't be dragged (only clicked to scroll into view).
                let is_stacked = ctx.panes.with_untracked(|p| {
                    p.iter()
                        .find(|ps| ps.id == drag.pane_id)
                        .is_some_and(|ps| ps.stack_side.is_some())
                });
                if is_stacked {
                    drag.last_pointer_x = Some(pos.x);
                    drag.last_pointer_y = Some(pos.y);
                    ctx.dragging.set(Some(drag));
                    return;
                }

                // Record start position on the first move event.
                if drag.last_pointer_x.is_none() {
                    drag.start_pointer_x = pos.x;
                    drag.start_pointer_y = pos.y;
                }
                if let (Some(lx), Some(ly)) = (drag.last_pointer_x, drag.last_pointer_y) {
                    let dx = pos.x - lx;
                    let dy = pos.y - ly;
                    // Dead zone: only consider it a drag once past the threshold.
                    // This prevents accidental drags when the user just wants to click.
                    if !drag.moved {
                        let dist = (pos.x - drag.start_pointer_x).abs()
                            + (pos.y - drag.start_pointer_y).abs();
                        if dist < DRAG_DEAD_ZONE {
                            drag.last_pointer_x = Some(pos.x);
                            drag.last_pointer_y = Some(pos.y);
                            ctx.dragging.set(Some(drag));
                            return;
                        }
                        drag.moved = true;
                    }
                    let (ww, wh) = ctx.window_size.get_untracked();
                    let is_browser_docked = ctx.panes.with_untracked(|p| {
                        p.iter()
                            .find(|ps| ps.id == drag.pane_id)
                            .map_or(false, |ps| matches!(ps.kind, PaneKind::Browser) && ps.docked)
                    });
                    ctx.panes.update(|p| {
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == drag.pane_id) {
                            // Browser pane while docked: only vertical movement (undock/redock).
                            // This prevents the browser from being horizontally repositioned
                            // since it's always pinned to the right edge.
                            if !is_browser_docked {
                                pane.x += dx;
                                pane.target_x = pane.x;
                            }

                            // Dock/undock logic based on vertical displacement.
                            if pane.docked {
                                let dock_y = pane.dock_y(wh);
                                // pane.y can go stale when display height changes
                                // (e.g., collapse toggled while docked). Snap it back
                                // so the undock threshold works from the visual position.
                                if pane.y < dock_y - UNDOCK_THRESHOLD {
                                    pane.y = dock_y;
                                }
                                pane.y += dy;
                                // Undock if dragged far enough above the dock position.
                                if dock_y - pane.y > UNDOCK_THRESHOLD {
                                    pane.docked = false;
                                } else {
                                    // Clamp to dock position (don't let it go below).
                                    pane.y = pane.y.min(dock_y);
                                }
                            } else {
                                pane.y += dy;
                                let dock_y = pane.dock_y(wh);
                                // Re-dock if dragged close to the bottom edge.
                                if pane.y >= dock_y - PANE_SPACING {
                                    pane.docked = true;
                                    pane.y = dock_y;
                                }
                            }
                        }
                        // Re-layout other panes to make room for the dragged one.
                        if !is_browser_docked {
                            recompute_targets_during_drag(p, drag.pane_id, ww, &mut drag.last_insert_pos);
                        }
                    });
                    ctx.anim_tick.set(ctx.anim_tick.get_untracked() + 1);
                    ctx.start_animation();
                }
                drag.last_pointer_x = Some(pos.x);
                drag.last_pointer_y = Some(pos.y);
                ctx.dragging.set(Some(drag));
                return;
            }

            // No active drag or resize — nothing to do on pointer move.
        })
        // --- Pointer up handler ---
        // Finalizes resize, drag, or toggle-collapse operations.
        .on_event_cont(listener::PointerUp, move |_, _| {
            // --- Resize end ---
            if ctx.resizing.get_untracked().is_some() {
                let (ww, _) = ctx.window_size.get_untracked();
                let fid = ctx.focus_pane_id.get_untracked();
                // Final re-layout to clean up positions.
                ctx.panes.update(|p| recompute_dock_targets(p, ww, fid));
                ctx.resizing.set(None);
                ctx.start_animation();
                return;
            }

            // --- Drag end ---
            if let Some(drag) = ctx.dragging.get_untracked() {
                if drag.moved {
                    // Actual drag: commit the new position.
                    let (ww, wh) = ctx.window_size.get_untracked();
                    ctx.focus_pane_id.set(Some(drag.pane_id));
                    ctx.panes.update(|p| {
                        // Snap-to-dock: if the pane was released near the bottom edge,
                        // dock it automatically.
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == drag.pane_id) {
                            if !pane.docked {
                                let dock_y = pane.dock_y(wh);
                                if pane.y >= dock_y - UNDOCK_THRESHOLD {
                                    pane.docked = true;
                                    pane.y = dock_y;
                                }
                            }
                        }
                        // Commit the spatial ordering produced by the drag
                        // into dock_order so panes don't revert on re-layout.
                        commit_drag_order(p, drag.pane_id);
                        recompute_dock_targets(p, ww, Some(drag.pane_id));
                    });
                } else {
                    // Click without movement: context-dependent behavior.
                    let is_stacked = ctx.panes
                        .get_untracked()
                        .iter()
                        .find(|p| p.id == drag.pane_id)
                        .is_some_and(|p| p.stack_side.is_some());
                    if is_stacked {
                        // Stacked pane: scroll it into view by focusing it.
                        ctx.focus_pane_id.set(Some(drag.pane_id));
                        let (ww, _) = ctx.window_size.get_untracked();
                        ctx.panes.update(|p| {
                            recompute_dock_targets(p, ww, Some(drag.pane_id));
                        });
                    } else {
                        // Visible pane: first click focuses, second click collapses.
                        if drag.was_focused {
                            // Already focused — toggle collapsed state and width.
                            let (ww, wh) = ctx.window_size.get_untracked();
                            ctx.panes.update(|p| {
                                if let Some(pane) =
                                    p.iter_mut().find(|ps| ps.id == drag.pane_id)
                                {
                                    pane.collapsed = !pane.collapsed;
                                    if pane.collapsed {
                                        // Save full width and shrink to collapsed width.
                                        pane.uncollapsed_width = pane.width;
                                        pane.width = COLLAPSED_PANE_WIDTH.min(pane.width);
                                    } else if pane.uncollapsed_width > 0.0 {
                                        // Restore pre-collapse width.
                                        pane.width = pane.uncollapsed_width;
                                        pane.uncollapsed_width = 0.0;
                                    }
                                    // Sync pane.y to the new visual dock position so it
                                    // doesn't go stale (display height changed).
                                    if pane.docked {
                                        pane.y = pane.dock_y(wh);
                                    }
                                }
                                recompute_dock_targets(p, ww, Some(drag.pane_id));
                            });
                        } else {
                            // First click on unfocused pane: just focus it
                            // (and focus its text input for chat panes).
                            let cid = ctx.panes.with_untracked(|p| {
                                p.iter()
                                    .find(|ps| ps.id == drag.pane_id)
                                    .and_then(|ps| ps.kind.channel_id())
                            });
                            if let Some(cid) = cid {
                                if let Some(trigger) = ctx.focus_triggers.with_untracked(|m| m.get(&cid).copied()) {
                                    trigger.update(|v| *v += 1);
                                }
                            }
                        }
                    }
                }
                ctx.dragging.set(None);
                ctx.start_animation();
            }
        })
        // Set the OS window title (shown in taskbar/dock even for undecorated windows).
        .window_title(|| "Paned Demo".to_string())
        // `keyboard_navigable()` makes the view focusable so it can receive
        // KeyDown events. Without this, keyboard events would go to whatever
        // child view has focus, and the root handler wouldn't fire.
        .style(|s| s.keyboard_navigable())
        // --- Keyboard shortcut handler ---
        // Cmd-W (Mac) / Ctrl-W (other) closes the focused pane.
        .on_event_stop(listener::KeyDown, move |_, KeyboardEvent { key, modifiers, .. }| {
            #[cfg(target_os = "macos")]
            let close_mod = Modifiers::META;
            #[cfg(not(target_os = "macos"))]
            let close_mod = Modifiers::CONTROL;

            if *key == Key::Character("w".into())
                && modifiers.contains(close_mod)
            {
                if let Some(pid) = ctx.focus_pane_id.get_untracked() {
                    let (ww, _) = ctx.window_size.get_untracked();
                    // Pick the next pane to focus after closing.
                    let new_focus = ctx.panes.with_untracked(|p| {
                        p.iter()
                            .filter(|ps| ps.id != pid)
                            .max_by_key(|ps| ps.dock_order)
                            .map(|ps| ps.id)
                    });
                    ctx.focus_pane_id.set(new_focus);
                    ctx.panes.update(|p| {
                        p.retain(|ps| ps.id != pid);
                        recompute_dock_targets(p, ww, new_focus);
                    });
                    ctx.pane_version.set(ctx.pane_version.get_untracked() + 1);
                    ctx.start_animation();
                }
            }
        })
}

fn main() {
    floem::Application::new()
        .window(
            |_| app_view(),
            Some(
                // Undecorated: no OS title bar (we provide our own toolbar).
                // Transparent: enables pseudo-window mode where the background
                // is see-through and only pane regions receive input.
                WindowConfig::default()
                    .size((1200., WINDOW_HEIGHT))
                    .with_transparent(true)
                    .undecorated(true),
            ),
        )
        .run();
}
