//! Native multi-window paned demo — identical pane behavior to `paned.rs`
//! but using real OS windows instead of a single hacked window.
//!
//! ## Layout
//!
//! - A small **management window** (decorated, normal z-order) determines
//!   which monitor the panes appear on and provides a button to reopen the
//!   browser pane if it was closed.
//! - Pane windows are **undecorated, always-on-top**, aligned to the
//!   bottom-right of the management window's monitor, spanning its full
//!   width. If the OS taskbar is at the bottom, panes are raised above it.
//!
//! ## Limitations
//!
//! - Requires window positioning support (macOS, X11, Windows). Does NOT
//!   work on Wayland — use `paned` instead.
//! - Cross-window drag reorder uses per-window PointerMove handlers
//!   rather than a single root PointerMove handler.

use std::collections::HashMap;

use floem::prelude::*;
use floem::reactive::Effect;
use floem::style::CursorStyle;
use floem::window::{WindowConfig, WindowIdExt};

use floem_demo::data::AppState;
use floem_demo::pane::PaneCtx;
use floem_demo::pane::layout::recompute_dock_targets;
use floem_demo::pane::model::*;
use floem_demo::pane::native::sync_windows;
use floem_demo::theme;

/// Fixed offset for the macOS menu bar at the top of the screen.
#[cfg(target_os = "macos")]
const MENU_BAR_HEIGHT: f64 = 25.0;
#[cfg(not(target_os = "macos"))]
const MENU_BAR_HEIGHT: f64 = 0.0;

/// Assumed OS taskbar/dock height when auto-detection is unavailable.
const TASKBAR_FALLBACK_HEIGHT: f64 = 70.0;

fn main() {
    let state = AppState::with_sample_data();
    let servers = state.servers;
    let channels = state.channels;
    let messages = state.messages;
    let active_server = state.active_server;
    let next_message_id = state.next_message_id;

    // Start with placeholder dimensions; will be replaced once the
    // management window tells us the actual monitor size.
    let initial_width = 1200.0;
    let initial_x = initial_width - BROWSER_PANE_WIDTH - PANE_SPACING;

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
            collapse_width: 0.0,
            collapse_side: None,
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
        window_ids: RwSignal::new(HashMap::new()),
        anchor_origin: RwSignal::new((0.0, 0.0)),
        native_mode: RwSignal::new(true),
        configured: RwSignal::new(false),
        anchor_view: RwSignal::new(None),
    };

    floem::Application::new()
        .window(
            move |mgmt_window_id| {
                // Reactive sync: spawn/close pane windows when panes change.
                // Guarded by `configured` so pane windows are only created
                // after configure_from_monitor has detected the monitor and
                // set correct positions. configure_from_monitor bumps
                // pane_version on its first run to trigger this Effect.
                Effect::new(move |_prev: Option<()>| {
                    ctx.pane_version.get();
                    if !ctx.configured.get_untracked() {
                        return;
                    }
                    sync_windows(
                        ctx, servers, channels, messages,
                        next_message_id, active_server,
                    );
                });

                // Reconfigure if the management window moves (or is first
                // placed). Also try on WindowResized which fires on initial
                // display — this is our main path for getting correct
                // monitor-derived positions.
                let mgmt_view = management_view(ctx)
                    .on_event_cont(
                        listener::WindowResized,
                        move |_, _: &floem::kurbo::Size| {
                            configure_from_monitor(mgmt_window_id, ctx);
                            floem_demo::pane::native::reposition_all_windows(ctx);
                        },
                    )
                    .on_event_cont(
                        listener::WindowMoved,
                        move |_, _pos: &floem::kurbo::Point| {
                            configure_from_monitor(mgmt_window_id, ctx);
                            floem_demo::pane::native::reposition_all_windows(ctx);
                        },
                    )
                    // Intercept Cmd-W on the management window: close the
                    // focused pane instead of closing the management window.
                    .style(|s| s.keyboard_navigable())
                    .on_event_stop(
                        listener::KeyDown,
                        move |_, KeyboardEvent { key, modifiers, .. }| {
                            #[cfg(target_os = "macos")]
                            let close_mod = Modifiers::META;
                            #[cfg(not(target_os = "macos"))]
                            let close_mod = Modifiers::CONTROL;

                            if *key == Key::Character("w".into())
                                && modifiers.contains(close_mod)
                            {
                                ctx.close_focused_pane();
                            }
                        },
                    )
                    .window_title(|| "Pane Manager".to_string());

                // Anchor the animation system to this long-lived window.
                ctx.anchor_view.set(Some(mgmt_view.view_id()));

                mgmt_view
            },
            Some(
                WindowConfig::default()
                    .size((260., 48.)),
            ),
        )
        .run();
}

/// Read the monitor that `mgmt_window_id` is on and set `anchor_origin`
/// and `window_size` so panes span the full monitor width and dock to
/// the bottom edge, raised above the OS taskbar if present.
fn configure_from_monitor(
    mgmt_window_id: floem::window::WindowId,
    ctx: PaneCtx,
) {
    let Some(layout) = mgmt_window_id.screen_layout() else {
        return;
    };
    let mon = layout.monitor_bounds;

    // `monitor_bounds` is the full physical display. We carve out
    // fixed offsets for the menu bar (top) and dock/taskbar (bottom).
    // The management window's position determines which MONITOR to
    // use, not where panes start — panes always fill from below the
    // menu bar to above the dock.
    let work_top = mon.y0 + MENU_BAR_HEIGHT;
    let work_bottom = mon.y1 - TASKBAR_FALLBACK_HEIGHT;
    let available_height = (work_bottom - work_top).max(200.0);

    let anchor_x = mon.x0;
    let anchor_y = work_top;

    ctx.anchor_origin.set((anchor_x, anchor_y));
    ctx.window_size.set((mon.width(), available_height));

    // Re-layout panes for the new dimensions.
    // Pane heights stay at their current value (DEFAULT_PANE_HEIGHT for
    // new panes).  The virtual window_size height determines docking
    // position — panes dock to the bottom of the available area.
    let fid = ctx.focus_pane_id.get_untracked();
    ctx.panes.update(|p| {
        for pane in p.iter_mut() {
            if pane.docked {
                pane.y = available_height - pane.display_height();
            }
        }
        recompute_dock_targets(p, mon.width(), fid);
        // Snap current positions to targets so the first frame renders
        // at the correct location (no animation needed for initial placement).
        for pane in p.iter_mut() {
            pane.x = pane.target_x;
        }
    });
    let first_time = !ctx.configured.get_untracked();
    ctx.configured.set(true);
    if first_time {
        // Trigger the Effect to create pane windows now that we have
        // correct monitor-derived positions.
        ctx.pane_version.set(ctx.pane_version.get_untracked() + 1);
    }
    ctx.start_animation();
}

/// Small management window view: just a "☰ Servers" button and a close button.
/// This window's position determines which monitor the panes appear on.
fn management_view(ctx: PaneCtx) -> impl IntoView {
    let browser_exists = move || {
        ctx.pane_version.get();
        ctx.panes.with_untracked(|p| {
            p.iter().any(|ps| matches!(ps.kind, PaneKind::Browser))
        })
    };

    let show_servers_btn = Label::new("☰ Servers")
        .style(move |s| {
            let exists = browser_exists();
            s.padding_horiz(12.0)
                .padding_vert(6.0)
                .font_size(14.0)
                .color(theme::TEXT_PRIMARY)
                .background(if exists { theme::ACTIVE_BG } else { theme::BLURPLE })
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .hover(|s| s.background(Color::from_rgb8(100, 120, 200)))
        })
        .on_event_stop(listener::Click, move |_, _| {
            open_or_focus_browser(ctx);
        });

    let close_app_btn = Label::new("✕")
        .style(|s| {
            s.font_size(16.0)
                .color(theme::TEXT_MUTED)
                .padding(6.0)
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .hover(|s| s.color(theme::TEXT_PRIMARY).background(theme::HOVER_BG))
        })
        .on_event_stop(listener::Click, move |_, _| {
            floem::quit_app();
        });

    let spacer = Empty::new().style(|s| s.flex_grow(1.0));

    Stack::horizontal((show_servers_btn, spacer, close_app_btn))
        .style(|s| {
            s.width_full()
                .height_full()
                .padding_horiz(8.0)
                .col_gap(8.0)
                .items_center()
                .background(theme::PANE_HEADER_BG)
        })
}

/// Open the browser pane or bring it into focus if it already exists.
fn open_or_focus_browser(ctx: PaneCtx) {
    let existing_id = ctx.panes.with_untracked(|p| {
        p.iter()
            .find(|ps| matches!(ps.kind, PaneKind::Browser))
            .map(|ps| ps.id)
    });
    if let Some(eid) = existing_id {
        ctx.focus_pane_id.set(Some(eid));
        let (ww, _) = ctx.window_size.get_untracked();
        ctx.panes.update(|p| recompute_dock_targets(p, ww, Some(eid)));
        ctx.start_animation();
        return;
    }
    let pid = ctx.next_pane_id.get_untracked();
    ctx.next_pane_id.set(pid + 1);
    let (ww, wh) = ctx.window_size.get_untracked();
    ctx.focus_pane_id.set(Some(pid));
    ctx.panes.update(|p| {
        p.push(PaneState {
            id: pid,
            kind: PaneKind::Browser,
            x: -BROWSER_PANE_WIDTH,
            target_x: -BROWSER_PANE_WIDTH,
            width: BROWSER_PANE_WIDTH,
            height: DEFAULT_PANE_HEIGHT,
            docked: true,
            y: wh - DEFAULT_PANE_HEIGHT,
            collapsed: false,
            uncollapsed_width: 0.0,
            dock_order: pid,
            stack_side: None,
            z_order: 0,
            collapse_width: 0.0,
            collapse_side: None,
        });
        recompute_dock_targets(p, ww, Some(pid));
    });
    ctx.pane_version.set(ctx.pane_version.get_untracked() + 1);
    ctx.start_animation();
}

