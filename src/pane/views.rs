//! UI components for the paned window manager: toolbar, pane cards, and browser content.
//!
//! ## Floem concepts demonstrated
//!
//! ### `dyn_stack` — dynamic, key-diffed list of views
//!
//! `dyn_stack(data_fn, key_fn, view_fn)` is the non-virtualized sibling of
//! `VirtualStack`. It renders *all* items (no viewport culling) and diffs
//! by key on each reactive update to add/remove/reorder views. Used here
//! for the pane list and server/channel lists inside the browser pane.
//!
//! Unlike `VirtualStack`, `dyn_stack` is suitable when:
//! - The list is small (< 50 items).
//! - Items have complex, variable-size layouts.
//! - You need absolute positioning (panes are positioned with `inset_left`/`inset_top`).
//!
//! ### Absolute positioning
//!
//! Floem supports both flexbox layout (the default) and absolute positioning.
//! `.absolute()` takes the element out of flow; `.inset_left(x)` and
//! `.inset_top(y)` set its position relative to the nearest positioned ancestor.
//! The pane cards use this for free-form placement controlled by the
//! layout algorithm.
//!
//! ### `.clip()` — overflow clipping
//!
//! `.clip()` prevents child content from rendering outside the element's
//! bounds. Essential for pane cards where the full content area is hidden
//! when the pane is stacked (showing only the tab overlay).
//!
//! ### `z_index` — render ordering
//!
//! `.z_index(i32)` controls which overlapping elements render on top.
//! The layout algorithm assigns z_order values:
//! - 0: default (no stacking)
//! - 100+: stacked peek strips (higher = closer to visible area)
//! - 200: fully visible panes
//! - 300: currently dragged pane
//!
//! ### Platform actions
//!
//! - `floem::action::drag_window()` — initiates a native window drag,
//!   allowing the user to move the undecorated window.
//! - `set_window_level(WindowLevel::AlwaysOnTop)` — pins the window above
//!   other windows.
//! - `floem::quit_app()` — exits the application.

use std::collections::HashMap;

use floem::action::{set_input_regions, set_window_level};
use floem::prelude::*;
use floem::style::{AnchorAbout, CursorStyle};
use floem::views::{ClipExt, Decorators, Empty, dyn_stack};
use floem::window::WindowLevel;

use crate::chat_area::chat_area_contents;
use crate::components::{channel_item, icon_circle, mini_server_icon, pane_header};
use crate::data::{Channel, Message, Server, send_message};
use crate::theme;

use super::PaneCtx;
use super::layout::{
    commit_drag_order, recompute_dock_targets, recompute_targets_during_drag, update_input_regions,
};
use super::model::*;
use super::native::reposition_single_window;

// ---------------------------------------------------------------------------
// Pane label content — shared header/tab label builder
// ---------------------------------------------------------------------------

/// Builds the label content used in pane headers and tab overlays.
///
/// This was extracted during refactoring to deduplicate the header
/// construction logic that was previously copied in both the pane header
/// and the stacked-tab overlay.
///
/// For the browser pane (``channel_id == None``), returns a "Servers" label.
/// For a chat pane, returns a mini server icon + "# channel-name".
///
/// ## `reactive` parameter
///
/// When ``reactive`` is true, the channel name updates via `Label::derived`
/// (used in the pane header where the name should update if channels are
/// renamed). When false, a static snapshot via `Label::new` is used (for
/// the tab overlay where reactivity adds overhead with no user benefit,
/// since the tab is only visible as a narrow strip).
pub fn pane_label_content(
    channel_id: Option<usize>,
    channels: RwSignal<Vec<Channel>>,
    servers: RwSignal<Vec<Server>>,
    reactive: bool,
) -> impl IntoView {
    match channel_id {
        None => Label::new("Servers")
            .style(|s| {
                s.font_size(14.0)
                    .font_weight(floem::text::FontWeight::BOLD)
                    .color(theme::TEXT_PRIMARY)
            })
            .into_any(),
        Some(cid) => {
            // Look up the server info for the mini icon. Uses `get_untracked`
            // because this is initialization-time data, not a reactive binding.
            let server_info: Option<(char, (u8, u8, u8))> = {
                let chs = channels.get_untracked();
                let svs = servers.get_untracked();
                chs.iter()
                    .find(|c| c.id == cid)
                    .and_then(|ch| svs.iter().find(|s| s.id == ch.server_id))
                    .map(|s| (s.icon_letter, s.color_rgb))
            };

            let channel_label = if reactive {
                // `Label::derived` re-evaluates the closure whenever
                // `channels.get()` changes (e.g. if a channel is renamed).
                Label::derived(move || {
                    let name = channels
                        .get()
                        .iter()
                        .find(|c| c.id == cid)
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    format!("# {name}")
                })
                .style(|s| {
                    s.font_size(14.0)
                        .font_weight(floem::text::FontWeight::BOLD)
                        .color(theme::TEXT_PRIMARY)
                })
                .into_any()
            } else {
                // Static snapshot — no reactive subscription, lighter weight.
                let name = channels
                    .get_untracked()
                    .iter()
                    .find(|c| c.id == cid)
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                Label::new(format!("# {name}"))
                    .style(|s| {
                        s.font_size(14.0)
                            .font_weight(floem::text::FontWeight::BOLD)
                            .color(theme::TEXT_PRIMARY)
                    })
                    .into_any()
            };

            if let Some((letter, (r, g, b))) = server_info {
                let icon = mini_server_icon(letter, Color::from_rgb8(r, g, b));
                Stack::horizontal((icon, channel_label))
                    .style(|s| s.items_center())
                    .into_any()
            } else {
                channel_label
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Browser pane content — server icons (left) + channel list (right)
// ---------------------------------------------------------------------------

/// Builds the interior of the server/channel browser pane: a narrow vertical
/// strip of server icons on the left, and a scrollable channel list for the
/// active server on the right. Clicking a channel calls
/// ``on_open_channel(channel_id)``.
///
/// ## `dyn_stack` vs `VirtualStack`
///
/// We use `dyn_stack` here instead of `VirtualStack` because:
/// 1. The server list is tiny (2 servers) — no need for virtualization.
/// 2. The channel list is small (~8 channels per server).
/// 3. `dyn_stack` integrates better with the pane's absolute positioning.
pub fn browser_content(
    servers: RwSignal<Vec<Server>>,
    channels: RwSignal<Vec<Channel>>,
    active_server: RwSignal<usize>,
    panes: RwSignal<Vec<PaneState>>,
    on_open_channel: impl Fn(usize) + 'static + Copy,
) -> impl IntoView {
    // Server icon column — same selection ring as server_list.rs
    // but smaller (28px icons instead of 42px).
    let server_col = dyn_stack(
        // Data source: `servers.get()` subscribes this stack to the signal.
        move || servers.get(),
        // Key function for diffing.
        |s: &Server| s.id,
        move |server: Server| {
            let sid = server.id;
            let is_active = move || active_server.get() == sid;
            icon_circle(
                server.icon_letter,
                server.color(),
                28.0,
                is_active,
                move || active_server.set(sid),
            )
            .container()
            .style(move |s| {
                let active = is_active();
                s.justify_center()
                    .items_center()
                    .padding(6.0)
                    .margin_bottom(2.0)
                    .border_right(if active { 1.5 } else { 0.0 })
                    .border_color(theme::BLURPLE)
            })
        },
    )
    .style(|s| s.flex_col().padding_top(4.0).items_center())
    .scroll()
    .style(|s| {
        s.width(44.0)
            .min_width(44.0)
            .flex_shrink(0.0)
            .height_full()
            .background(theme::SERVER_BAR_BG)
    });

    // Channel list for the active server.
    let channel_list = dyn_stack(
        move || {
            let sid = active_server.get();
            channels
                .get()
                .into_iter()
                .filter(move |c| c.server_id == sid)
                .collect::<Vec<_>>()
        },
        |ch: &Channel| ch.id,
        move |ch: Channel| {
            let cid = ch.id;
            // Highlight channels that already have an open pane.
            // `panes.get()` subscribes so highlights update when panes open/close.
            let is_open = move || {
                panes
                    .get()
                    .iter()
                    .any(|p| p.kind.channel_id() == Some(cid))
            };
            channel_item(ch.name, is_open, move || on_open_channel(cid))
        },
    )
    .style(|s| s.flex_col().width_full().padding_horiz(8.0).padding_top(4.0))
    .scroll()
    .style(|s| s.flex_grow(1.0).height_full());

    // Side-by-side layout: server icons on the left, channel list on the right.
    Stack::horizontal((server_col, channel_list))
        .style(|s| s.width_full().flex_grow(1.0))
}

// ---------------------------------------------------------------------------
// Toolbar — top bar with window controls and pane actions
// ---------------------------------------------------------------------------

/// The toolbar strip at the top of the paned window.
///
/// Contains:
/// - Drag grip (⠿) — initiates native window drag via `drag_window()`.
/// - "☰ Servers" button — opens or focuses the browser pane.
/// - Spacer — pushes right-side buttons to the right edge.
/// - PW toggle — enables/disables pseudo-window mode (transparent click-through).
/// - PIN toggle — pins the window above other windows.
/// - Close button — quits the application.
///
/// ## `floem::action::drag_window()`
///
/// Since this is an undecorated window (no title bar), we need to provide
/// our own drag handle. `drag_window()` tells the OS to begin a native
/// window drag from the current pointer position. The grip handle uses
/// `PointerDown` (not `Click`) because the OS takes over pointer tracking
/// immediately.
pub fn toolbar(
    ctx: PaneCtx,
) -> impl IntoView {
    // Show the "☰ Servers" button when the browser pane is closed OR stacked.
    // Reads both `pane_version` (for add/remove) and `anim_tick` (for stack
    // state changes during animation). Uses `with_untracked` for the actual
    // data to avoid cloning the pane Vec.
    let browser_visible = move || {
        ctx.pane_version.get();
        ctx.anim_tick.get();
        ctx.panes.with_untracked(|p| {
            p.iter()
                .find(|ps| matches!(ps.kind, PaneKind::Browser))
                .is_some_and(|ps| ps.stack_side.is_none())
        })
    };

    let show_servers_btn = Label::new("☰ Servers")
        .style(move |s| {
            let vis = !browser_visible();
            s.padding_horiz(12.0)
                .padding_vert(6.0)
                .font_size(14.0)
                .color(theme::TEXT_PRIMARY)
                .background(theme::BLURPLE)
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .hover(|s| s.background(Color::from_rgb8(100, 120, 200)))
                // Conditionally hide/show. `Display::None` removes the element
                // from layout entirely (unlike `visibility: hidden` in CSS
                // which just makes it invisible but reserves space).
                .display(if vis {
                    floem::style::Display::Flex
                } else {
                    floem::style::Display::None
                })
        })
        .on_event_stop(listener::Click, move |_, _| {
            // If the browser pane exists but is stacked, bring it into focus
            // by setting focus_pane_id and re-running the layout algorithm.
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
            // No browser pane exists — create a new one.
            let pid = ctx.next_pane_id.get_untracked();
            ctx.next_pane_id.set(pid + 1);
            let (ww, wh) = ctx.window_size.get_untracked();
            ctx.focus_pane_id.set(Some(pid));
            ctx.panes.update(|p| {
                // Start off-screen to the left so it animates in.
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
                // Recompute layout with the new pane as focus.
                recompute_dock_targets(p, ww, Some(pid));
            });
            // Bump pane_version so dyn_stack re-diffs and creates the view.
            ctx.pane_version.set(ctx.pane_version.get_untracked() + 1);
            ctx.start_animation();
        });

    // Drag grip: the braille dots character (⠿) serves as a visual handle.
    let drag_grip = Label::new("⠿")
        .style(|s| {
            s.font_size(18.0)
                .color(theme::TEXT_MUTED)
                .padding_horiz(8.0)
                .cursor(CursorStyle::Grab)
                .hover(|s| s.color(theme::TEXT_PRIMARY))
        })
        .on_event_stop(listener::PointerDown, |_, _| {
            // `drag_window()` initiates a native OS window drag.
            // The OS takes over pointer tracking from here.
            floem::action::drag_window();
        });

    let close_app_btn = Label::new("x")
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

    // Pin button: toggles always-on-top via `set_window_level`.
    // Uses "PIN"/"pin" text to distinguish active/inactive state.
    let pinned = RwSignal::new(false);
    let pin_btn = Label::derived(move || if pinned.get() { "PIN" } else { "pin" })
        .style(move |s| {
            let active = pinned.get();
            s.font_size(14.0)
                .font_weight(floem::text::FontWeight::BOLD)
                .padding(6.0)
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .color(if active { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED })
                .background(if active { theme::ACTIVE_BG } else { Color::TRANSPARENT })
                .hover(|s| s.background(theme::HOVER_BG))
        })
        .on_event_stop(listener::Click, move |_, _| {
            let new_val = !pinned.get_untracked();
            pinned.set(new_val);
            // `set_window_level` is a Floem action that maps to the OS
            // window manager's always-on-top functionality.
            let level = if new_val {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            };
            set_window_level(level);
        });

    // Pseudo-window mode toggle: transparent background with per-region click-through.
    // When on, only panes and the toolbar receive mouse events; clicks on the
    // transparent background pass through to the desktop.
    let pseudo_window_btn = Label::derived(move || {
            if ctx.pseudo_window.get() { "PW" } else { "pw" }
        })
        .style(move |s| {
            let active = ctx.pseudo_window.get();
            s.font_size(14.0)
                .font_weight(floem::text::FontWeight::BOLD)
                .padding(6.0)
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .color(if active { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED })
                .background(if active { theme::ACTIVE_BG } else { Color::TRANSPARENT })
                .hover(|s| s.background(theme::HOVER_BG))
        })
        .on_event_stop(listener::Click, move |_, _| {
            let new_val = !ctx.pseudo_window.get_untracked();
            ctx.pseudo_window.set(new_val);
            if new_val {
                // Enable click-through: push input regions so only panes are interactive.
                let (ww, wh) = ctx.window_size.get_untracked();
                ctx.panes.with_untracked(|p| update_input_regions(p, ww, wh));
            } else {
                // Disable click-through: whole window receives clicks.
                set_input_regions(None);
            }
        });

    // Spacer pushes the right-side buttons to the right edge.
    // `flex_grow(1.0)` on an empty view fills remaining horizontal space.
    let spacer = Empty::new().style(|s| s.flex_grow(1.0));

    Stack::horizontal((drag_grip, show_servers_btn, spacer, pseudo_window_btn, pin_btn, close_app_btn))
        .style(|s| {
            s.width_full()
                .height(TOOLBAR_HEIGHT)
                .padding_horiz(12.0)
                .col_gap(8.0)
                .items_center()
                .background(theme::PANE_HEADER_BG)
                .border_radius(8.0)
        })
        // Position the toolbar absolutely at the top of the window.
        // `absolute()` takes it out of the normal flexbox flow.
        .style(|s| {
            s.absolute()
                .inset_left(0.0)
                .inset_top(0.0)
                .inset_right(0.0)
        })
}

// ---------------------------------------------------------------------------
// Pane card — unified container for both browser and chat panes
// ---------------------------------------------------------------------------

/// The inner content of a pane card: header + body + resize handles + tab
/// overlay, clipped and styled with the pane's background and border.
///
/// This is the reusable core shared by both `pane_card` (single-window mode,
/// adds absolute positioning) and the native multi-window mode (each pane is
/// the root view of its own OS window — no positioning wrapper needed).
///
/// Returns a clipped `Stack` with `width_full().height_full()` — the caller
/// is responsible for sizing/positioning the container.
pub fn pane_content(
    pane_id: usize,
    kind: PaneKind,
    servers: RwSignal<Vec<Server>>,
    channels: RwSignal<Vec<Channel>>,
    messages: RwSignal<HashMap<usize, Vec<Message>>>,
    next_message_id: RwSignal<usize>,
    active_server: RwSignal<usize>,
    ctx: PaneCtx,
) -> impl IntoView {
    let channel_id_opt = kind.channel_id();

    // --- Close handler ---
    // Removes the pane, cleans up its focus trigger, and re-layouts.
    let on_close = move || {
        let (ww, _) = ctx.window_size.get_untracked();

        // Pick a new focus before removing so focus_pane_id always
        // points to a valid pane (or None if empty).
        let new_focus = if ctx.focus_pane_id.get_untracked() == Some(pane_id) {
            let nf = ctx.panes.with_untracked(|p| neighbor_focus(p, pane_id));
            ctx.focus_pane_id.set(nf);
            nf
        } else {
            ctx.focus_pane_id.get_untracked()
        };

        // Clean up the focus trigger for this channel (if it's a chat pane).
        if let Some(cid) = channel_id_opt {
            ctx.focus_triggers.update(|m| { m.remove(&cid); });
        }
        ctx.panes.update(|p| {
            p.retain(|ps| ps.id != pane_id);
            recompute_dock_targets(p, ww, new_focus);
        });
        // Bump pane_version so dyn_stack / native sync removes the view/window.
        ctx.pane_version.set(ctx.pane_version.get_untracked() + 1);
        ctx.start_animation();
    };

    // --- Drag start handler ---
    // Called when the user presses on the header. Sets up DragInfo with
    // initial state; actual drag movement is handled by the host binary's
    // PointerMove handler (paned.rs or native_paned.rs).
    let start_drag = move || {
        let was_focused = ctx.focus_pane_id.get_untracked() == Some(pane_id);
        ctx.focus_pane_id.set(Some(pane_id));
        ctx.pane_version.set(ctx.pane_version.get_untracked() + 1);
        ctx.dragging.set(Some(DragInfo {
            pane_id,
            start_pointer_x: 0.0,
            start_pointer_y: 0.0,
            last_pointer_x: None,
            last_pointer_y: None,
            moved: false,
            last_insert_pos: None,
            was_focused,
        }));
    };

    // --- Header ---
    // Uses reactive label (updates if channel is renamed).
    let header_content = pane_label_content(channel_id_opt, channels, servers, true);

    let header = pane_header(header_content, on_close)
        .style(|s| s.cursor(CursorStyle::Grab))
        // Reactive background: focused pane gets a lighter header.
        .style(move |s| {
            // Read pane_version to re-evaluate when focus changes.
            ctx.pane_version.get();
            let focused = ctx.focus_pane_id.get_untracked() == Some(pane_id);
            let bg = if focused {
                theme::PANE_HEADER_FOCUSED_BG
            } else {
                theme::PANE_HEADER_BG
            };
            s.background(bg)
        })
        // PointerDown (not Click) so we can distinguish click from drag
        // based on subsequent movement.
        .on_event_stop(listener::PointerDown, move |_, _| {
            start_drag();
        });

    // --- Pane body content ---
    let content = if let Some(cid) = channel_id_opt {
        // Chat pane: build a message timeline with input bar.
        let channel_name = move || {
            channels.with(|chs| {
                chs.iter()
                    .find(|c| c.id == cid)
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            })
        };
        // Borrow the HashMap instead of cloning it; only clone
        // the Vec for the active channel.
        let current_messages = move || {
            messages.with(|m| m.get(&cid).cloned().unwrap_or_default())
        };
        // Uses the shared `send_message` helper from data.rs, capturing
        // only the signals it needs (not the full AppState).
        let on_send = move |text: String| {
            send_message(messages, next_message_id, cid, text);
        };
        // Create a focus trigger for this channel's text input.
        let focus_input = RwSignal::new(0u64);
        ctx.focus_triggers.update(|m| { m.insert(cid, focus_input); });
        let panel_height = RwSignal::new(400.0f64);
        let (message_list, input) = chat_area_contents(channel_name, current_messages, on_send, focus_input, panel_height);
        Stack::vertical((message_list, input))
            .on_event_cont(floem::context::LayoutChanged::listener(), move |_cx, change| {
                panel_height.set(change.new_box.height());
            })
            .style(|s| s.width_full().flex_grow(1.0))
            .into_any()
    } else {
        // Browser pane — clicking a channel opens or focuses a chat pane.
        let on_open_channel = move |channel_id: usize| {
            // Check if a pane for this channel already exists.
            let existing_id = ctx.panes
                .get_untracked()
                .iter()
                .find(|p| p.kind.channel_id() == Some(channel_id))
                .map(|p| p.id);
            if let Some(eid) = existing_id {
                // Bring the existing pane into view, un-collapse it, and focus its text input.
                ctx.focus_pane_id.set(Some(eid));
                let (ww, _) = ctx.window_size.get_untracked();
                ctx.panes.update(|p| {
                    if let Some(pane) = p.iter_mut().find(|ps| ps.id == eid) {
                        pane.collapsed = false;
                        // Restore pre-collapse width if it was collapsed.
                        if pane.uncollapsed_width > 0.0 {
                            pane.width = pane.uncollapsed_width;
                            pane.uncollapsed_width = 0.0;
                        }
                    }
                    recompute_dock_targets(p, ww, Some(eid));
                });
                // Bump the focus trigger to focus the text input.
                if let Some(trigger) = ctx.focus_triggers.with_untracked(|m| m.get(&channel_id).copied()) {
                    trigger.update(|v| *v += 1);
                }
                ctx.start_animation();
                return;
            }
            // Create a new chat pane for this channel.
            let pid = ctx.next_pane_id.get_untracked();
            ctx.next_pane_id.set(pid + 1);
            let (ww, wh) = ctx.window_size.get_untracked();
            ctx.focus_pane_id.set(Some(pid));
            ctx.panes.update(|p| {
                let new_order = if OPEN_PANES_LEFT {
                    // Highest dock_order = leftmost position.
                    let max = p
                        .iter()
                        .filter(|ps| !matches!(ps.kind, PaneKind::Browser))
                        .map(|ps| ps.dock_order)
                        .max()
                        .unwrap_or(0);
                    max + 1
                } else {
                    // dock_order 0 = rightmost (adjacent to browser).
                    // Shift existing panes left to make room.
                    for existing in p.iter_mut() {
                        if !matches!(existing.kind, PaneKind::Browser) {
                            existing.dock_order += 1;
                        }
                    }
                    0
                };
                // Start off-screen to the left so it animates in.
                p.push(PaneState {
                    id: pid,
                    kind: PaneKind::Chat { channel_id },
                    x: -DEFAULT_PANE_WIDTH,
                    target_x: -DEFAULT_PANE_WIDTH,
                    width: DEFAULT_PANE_WIDTH,
                    height: DEFAULT_PANE_HEIGHT,
                    docked: true,
                    y: wh - DEFAULT_PANE_HEIGHT,
                    collapsed: false,
                    uncollapsed_width: 0.0,
                    dock_order: new_order,
                    stack_side: None,
                    z_order: 0,
                    collapse_width: 0.0,
                    collapse_side: None,
                });
                // Focus the new pane so the card-stack keeps it visible.
                recompute_dock_targets(p, ww, Some(pid));
            });
            ctx.pane_version.set(ctx.pane_version.get_untracked() + 1);
            ctx.start_animation();
        };
        browser_content(servers, channels, active_server, ctx.panes, on_open_channel).into_any()
    };

    // Use sidebar bg for browser pane, chat bg for chat panes.
    let bg = if channel_id_opt.is_some() {
        theme::CHAT_BG
    } else {
        theme::CHANNEL_SIDEBAR_BG
    };

    // --- Clipped content area ---
    // The main pane content (header + body), clipped to the pane bounds.
    // During the collapse animation the content stays at full pane width
    // (avoiding text re-layout) and is clipped by the shrinking outer
    // container. For right-stacking, a translate_x offset keeps the
    // content visually stationary while the pane origin shifts rightward.
    let clipped_content = Stack::vertical((header, content))
        .style(|s| s.width_full().height_full())
        // Clicking anywhere in the pane focuses it.
        // `on_event_cont` allows the click to also reach child views.
        .on_event_cont(listener::PointerDown, move |_, _| {
            if ctx.focus_pane_id.get_untracked() != Some(pane_id) {
                ctx.focus_pane_id.set(Some(pane_id));
                ctx.pane_version.set(ctx.pane_version.get_untracked() + 1);
            }
        })
        // `.clip()` prevents content from rendering outside the pane bounds.
        .clip()
        .style(move |s| {
            ctx.anim_tick.get();
            let (full_width, collapse_width, collapse_side, fully_collapsed, x, target_x) =
                ctx.panes.with_untracked(|ps| {
                    ps.iter()
                        .find(|p| p.id == pane_id)
                        .map(|p| (p.width, p.collapse_width, p.collapse_side,
                                  p.is_fully_collapsed(), p.x, p.target_x))
                        .unwrap_or((0.0, 0.0, None, false, 0.0, 0.0))
                });

            if fully_collapsed {
                return s.width(full_width).height_full().display(floem::style::Display::None);
            }

            // Keep the content at the pane's full logical width so that
            // text layout is never recalculated during the animation.
            let base = s.width(full_width).height_full();

            // Compute a translate_x that keeps content visually anchored
            // while the tab sweeps across it. The approach differs by side:
            //
            // Right-stacking: render_x = x + collapse_width, so content
            //   shifts by -collapse_width to stay at screen position x.
            //
            // Left-stacking: render_x = x (which is animating leftward).
            //   We derive the pane's traveled x-distance from collapse
            //   progress (both use the same easing) and compensate so the
            //   content appears stationary in screen space.
            match collapse_side {
                Some(StackSide::Right) => base.translate_x(-collapse_width),
                Some(StackSide::Left) => {
                    let remaining_collapse = full_width - PEEK_WIDTH - collapse_width;
                    let translate = if remaining_collapse > ANIM_SNAP {
                        -collapse_width * (target_x - x) / remaining_collapse
                    } else {
                        0.0
                    };
                    base.translate_x(translate)
                }
                None => base,
            }
        });

    // --- Tab overlay ---
    // Visible during the collapse/expand animation and when fully stacked.
    // During the animation the tab is a PEEK_WIDTH strip at the leading
    // edge of the collapse (opposite the stack direction), giving the
    // appearance of sweeping across the content.
    let tab_content = pane_label_content(channel_id_opt, channels, servers, false);
    let tab_overlay = Stack::horizontal((tab_content,))
        .style(move |s| {
            ctx.anim_tick.get();
            // Rotation direction and text alignment are based on collapse_side
            // so they persist through the expand animation after stack_side
            // is cleared.
            let pane_height = ctx.panes.with_untracked(|ps| {
                ps.iter()
                    .find(|p| p.id == pane_id)
                    .map(|p| p.display_height())
                    .unwrap_or(0.0)
            });

            // The inner element is laid out as a wide horizontal strip
            // (width = pane_height) then rotated +90° into the narrow
            // vertical tab. Left-aligned content maps to the visual top
            // after rotation, so the icon naturally sits at the top of
            // the strip for both stacking sides. The `rotate_about` pivot
            // is chosen so the rotated bounding box lands exactly within
            // (0, 0, PEEK_WIDTH, pane_height).
            let ph = pane_height.max(1.0);
            let anchor = AnchorAbout {
                x: PEEK_WIDTH / (2.0 * ph),
                y: 0.5,
            };
            s.absolute()
                .inset_left(0.0)
                .inset_top(0.0)
                .width(ph)
                .height(PEEK_WIDTH)
                .items_center()
                .justify_start()
                .padding_left(12.0)
                .rotate(90.0_f64.deg())
                .rotate_about(anchor)
        })
        .container()
        .style(move |s| {
            ctx.anim_tick.get();
            let (collapse_side, collapse_width, full_width) = ctx.panes.with_untracked(|ps| {
                ps.iter()
                    .find(|p| p.id == pane_id)
                    .map(|p| (p.collapse_side, p.collapse_width, p.width))
                    .unwrap_or((None, 0.0, 0.0))
            });
            let is_visible = collapse_side.is_some() && collapse_width > 0.0;
            if !is_visible {
                return s.display(floem::style::Display::None);
            }
            // PEEK_WIDTH strip positioned at the leading edge of the collapse.
            // Both sides use inset_left with a computed offset — inset_right
            // doesn't reliably position elements in floem's layout engine.
            let tab_left = match collapse_side {
                Some(StackSide::Left) => full_width - collapse_width - PEEK_WIDTH,
                _ => 0.0,
            };
            s.absolute()
                .inset_top(0.0)
                .inset_bottom(0.0)
                .inset_left(tab_left)
                .width(PEEK_WIDTH)
                .background(theme::PANE_HEADER_BG)
                .border_radius(8.0)
                .cursor(CursorStyle::Pointer)
        })
        // Clicking a stacked tab focuses that pane (scrolls it into view).
        .on_event_stop(listener::PointerDown, move |_, _| {
            start_drag();
        });

    // --- Resize handles ---
    // Invisible edge zones that start resize operations when clicked.
    // Side handles sit below the header to avoid conflicting with the
    // drag grip and close button.
    let left_handle = resize_handle(pane_id, ResizeEdge::Left, ctx.resizing);
    let right_handle = resize_handle(pane_id, ResizeEdge::Right, ctx.resizing);
    let top_handle = resize_handle(pane_id, ResizeEdge::Top, ctx.resizing);
    let top_left_handle = resize_handle(pane_id, ResizeEdge::TopLeft, ctx.resizing);
    let top_right_handle = resize_handle(pane_id, ResizeEdge::TopRight, ctx.resizing);

    // --- Inner container ---
    // Holds all visual elements; caller wraps with positioning as needed.
    Stack::new((
        clipped_content,
        left_handle,
        right_handle,
        top_handle,
        top_left_handle,
        top_right_handle,
        tab_overlay,
    ))
        .clip()
        .style(move |s| {
            s.width_full()
                .height_full()
                .background(bg)
                .border_radius(8.0)
                .border(1.0)
                .border_color(theme::PANE_BORDER)
        })
        // --- Native multi-window: handle drag and resize via per-window PointerMove ---
        // In single-window mode, the root PointerMove handler (paned.rs) drives
        // drag/resize. In native mode, each pane window handles its own events
        // and repositions windows via the animation system.
        .on_event_cont(listener::PointerMove, move |_, event| {
            if !ctx.is_native_mode() {
                return;
            }
            let pos = event.current.logical_point();

            // --- Resize ---
            if let Some(mut rz) = ctx.resizing.get_untracked() {
                if rz.pane_id != pane_id {
                    return;
                }
                let has_prev = rz.last_x.is_some() || rz.last_y.is_some();
                if has_prev {
                    let dx = rz.last_x.map(|lx| pos.x - lx).unwrap_or(0.0);
                    let dy = rz.last_y.map(|ly| pos.y - ly).unwrap_or(0.0);
                    let (ww, _) = ctx.window_size.get_untracked();
                    ctx.panes.update(|p| {
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == rz.pane_id) {
                            let min_w = pane.min_resize_width();
                            match rz.edge {
                                ResizeEdge::Right | ResizeEdge::TopRight => {
                                    pane.width = (pane.width + dx).max(min_w);
                                }
                                ResizeEdge::Left | ResizeEdge::TopLeft => {
                                    let new_w = (pane.width - dx).max(min_w);
                                    let delta = pane.width - new_w;
                                    pane.x += delta;
                                    pane.width = new_w;
                                }
                                ResizeEdge::Top => {}
                            }
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
                        let mut no_hysteresis = None;
                        recompute_targets_during_drag(p, rz.pane_id, ww, &mut no_hysteresis);
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == rz.pane_id) {
                            pane.target_x = pane.x;
                        }
                    });
                    ctx.anim_tick.set(ctx.anim_tick.get_untracked() + 1);
                    ctx.start_animation();
                }
                rz.last_x = Some(pos.x);
                rz.last_y = Some(pos.y);
                ctx.resizing.set(Some(rz));
                return;
            }

            // --- Drag ---
            // In native mode, the dragged pane's window is moved directly
            // here (not by the animation loop) so the local pointer
            // coordinates stay consistent. We record last_pointer only
            // once (first PointerMove) and never update it: each
            // set_window_inner_bounds shifts the local coordinate frame
            // by exactly the delta, so the stored baseline stays correct.
            if let Some(mut drag) = ctx.dragging.get_untracked() {
                if drag.pane_id != pane_id {
                    return;
                }
                let is_stacked = ctx.panes.with_untracked(|p| {
                    p.iter()
                        .find(|ps| ps.id == drag.pane_id)
                        .is_some_and(|ps| ps.stack_side.is_some())
                });
                if is_stacked {
                    if drag.last_pointer_x.is_none() {
                        drag.last_pointer_x = Some(pos.x);
                        drag.last_pointer_y = Some(pos.y);
                        ctx.dragging.set(Some(drag));
                    }
                    return;
                }

                // First PointerMove: record the baseline, no delta yet.
                if drag.last_pointer_x.is_none() {
                    drag.start_pointer_x = pos.x;
                    drag.start_pointer_y = pos.y;
                    drag.last_pointer_x = Some(pos.x);
                    drag.last_pointer_y = Some(pos.y);
                    ctx.dragging.set(Some(drag));
                    return;
                }
                let (lx, ly) = (drag.last_pointer_x.unwrap(), drag.last_pointer_y.unwrap());
                let dx = pos.x - lx;
                let dy = pos.y - ly;
                if !drag.moved {
                    let dist = (pos.x - drag.start_pointer_x).abs()
                        + (pos.y - drag.start_pointer_y).abs();
                    if dist < DRAG_DEAD_ZONE {
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
                        if !is_browser_docked {
                            pane.x += dx;
                            pane.target_x = pane.x;
                        }
                        if pane.docked {
                            let dock_y = pane.dock_y(wh);
                            if pane.y < dock_y - UNDOCK_THRESHOLD {
                                pane.y = dock_y;
                            }
                            pane.y += dy;
                            if dock_y - pane.y > UNDOCK_THRESHOLD {
                                pane.docked = false;
                            } else {
                                pane.y = pane.y.min(dock_y);
                            }
                        } else {
                            pane.y += dy;
                            let dock_y = pane.dock_y(wh);
                            if pane.y >= dock_y - PANE_SPACING {
                                pane.docked = true;
                                pane.y = dock_y;
                            }
                        }
                    }
                    if !is_browser_docked {
                        recompute_targets_during_drag(p, drag.pane_id, ww, &mut drag.last_insert_pos);
                    }
                });
                // Move the dragged window immediately (the animation loop
                // skips it to avoid coordinate-frame conflicts).
                reposition_single_window(ctx, drag.pane_id);
                ctx.anim_tick.set(ctx.anim_tick.get_untracked() + 1);
                ctx.start_animation();
                // Don't update last_pointer — the window move shifts
                // local coords by exactly the delta, keeping the
                // baseline correct for the next frame.
                ctx.dragging.set(Some(drag));
            }
        })
        // --- Native multi-window: finalize drag or resize on pointer up ---
        .on_event_cont(listener::PointerUp, move |_, _| {
            if !ctx.is_native_mode() {
                return;
            }
            // Resize end.
            if let Some(rz) = ctx.resizing.get_untracked() {
                if rz.pane_id == pane_id {
                    let (ww, _) = ctx.window_size.get_untracked();
                    let fid = ctx.focus_pane_id.get_untracked();
                    ctx.panes.update(|p| recompute_dock_targets(p, ww, fid));
                    ctx.resizing.set(None);
                    ctx.start_animation();
                    return;
                }
            }
            // Drag end.
            if let Some(drag) = ctx.dragging.get_untracked() {
                if drag.pane_id != pane_id {
                    return;
                }
                if drag.moved {
                    let (ww, wh) = ctx.window_size.get_untracked();
                    ctx.focus_pane_id.set(Some(drag.pane_id));
                    ctx.panes.update(|p| {
                        // Snap-to-dock if near bottom edge.
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == drag.pane_id) {
                            if !pane.docked {
                                let dock_y = pane.dock_y(wh);
                                if pane.y >= dock_y - UNDOCK_THRESHOLD {
                                    pane.docked = true;
                                    pane.y = dock_y;
                                }
                            }
                        }
                        commit_drag_order(p, drag.pane_id);
                        recompute_dock_targets(p, ww, Some(drag.pane_id));
                    });
                } else {
                    // Click (no movement) — same collapse/focus logic as paned.rs.
                    handle_pane_click(pane_id, &drag, ctx);
                }
                ctx.dragging.set(None);
                ctx.start_animation();
            }
        })
        // --- Close this pane with Cmd-W / Ctrl-W ---
        // The editor lets unhandled key combos bubble, so Cmd-W arrives
        // here even when a text input has focus.
        //
        // Uses on_event_cont so the event keeps bubbling in single-
        // window mode — the root view in paned.rs handles it there.
        // In native mode this IS the window root, so there's nothing
        // above to propagate to.
        .style(move |s| {
            if ctx.is_native_mode() {
                s.keyboard_navigable()
            } else {
                s
            }
        })
        .on_event_cont(listener::KeyDown, move |_, KeyboardEvent { key, modifiers, .. }| {
            if !ctx.is_native_mode() {
                return;
            }
            #[cfg(target_os = "macos")]
            let close_mod = Modifiers::META;
            #[cfg(not(target_os = "macos"))]
            let close_mod = Modifiers::CONTROL;

            if *key == Key::Character("w".into()) && modifiers.contains(close_mod) {
                on_close();
            }
        })
}

/// Handle a click (non-drag) on a pane header in native mode.
/// Mirrors the logic in `paned.rs`'s PointerUp handler.
fn handle_pane_click(pane_id: usize, drag: &DragInfo, ctx: PaneCtx) {
    let is_stacked = ctx.panes
        .get_untracked()
        .iter()
        .find(|p| p.id == pane_id)
        .is_some_and(|p| p.stack_side.is_some());

    if is_stacked {
        ctx.focus_pane_id.set(Some(pane_id));
        let (ww, _) = ctx.window_size.get_untracked();
        ctx.panes.update(|p| {
            recompute_dock_targets(p, ww, Some(pane_id));
        });
    } else if drag.was_focused {
        let (ww, wh) = ctx.window_size.get_untracked();
        ctx.panes.update(|p| {
            if let Some(pane) = p.iter_mut().find(|ps| ps.id == pane_id) {
                pane.collapsed = !pane.collapsed;
                if pane.collapsed {
                    pane.uncollapsed_width = pane.width;
                    pane.width = COLLAPSED_PANE_WIDTH.min(pane.width);
                } else if pane.uncollapsed_width > 0.0 {
                    pane.width = pane.uncollapsed_width;
                    pane.uncollapsed_width = 0.0;
                }
                if pane.docked {
                    pane.y = pane.dock_y(wh);
                }
            }
            recompute_dock_targets(p, ww, Some(pane_id));
        });
    } else {
        let cid = ctx.panes.with_untracked(|p| {
            p.iter()
                .find(|ps| ps.id == pane_id)
                .and_then(|ps| ps.kind.channel_id())
        });
        if let Some(cid) = cid {
            if let Some(trigger) =
                ctx.focus_triggers.with_untracked(|m| m.get(&cid).copied())
            {
                trigger.update(|v| *v += 1);
            }
        }
    }
}

/// A complete pane card: header + content + resize handles + tab overlay,
/// wrapped in an absolutely-positioned container for the single-window mode.
///
/// This is the main view factory called by `dyn_stack` in `paned.rs` for
/// each open pane. It delegates to [`pane_content`] for the inner content
/// and adds the absolute positioning + z-index wrapper that places the
/// pane at its animated `(x, y)` coordinates within the single window.
///
/// ## Signal subscription strategy
///
/// The outer style closure reads `anim_tick.get()` (a cheap u64) to
/// subscribe to animation updates, then uses `panes.with_untracked()` to
/// borrow the actual PaneState data without cloning. This is critical for
/// performance — cloning `Vec<PaneState>` on every animation frame (60fps)
/// would be wasteful.
pub fn pane_card(
    pane_id: usize,
    kind: PaneKind,
    servers: RwSignal<Vec<Server>>,
    channels: RwSignal<Vec<Channel>>,
    messages: RwSignal<HashMap<usize, Vec<Message>>>,
    next_message_id: RwSignal<usize>,
    active_server: RwSignal<usize>,
    ctx: PaneCtx,
) -> impl IntoView {
    let inner = pane_content(
        pane_id, kind, servers, channels, messages,
        next_message_id, active_server, ctx,
    );

    // Wrap the shared pane content in an absolute-positioning container
    // that places the card at its animated screen position within the
    // single parent window.
    inner
        .container()
        .style(move |s| {
            // Subscribe to the lightweight tick counter rather than the full
            // pane Vec — avoids cloning and diffing every animation frame.
            ctx.anim_tick.get();
            let (_, wh) = ctx.window_size.get();
            // Borrow-without-clone: find this pane's current state.
            let found = ctx.panes.with_untracked(|ps| {
                ps.iter().find(|p| p.id == pane_id).cloned()
            });
            if let Some(p) = found {
                s.absolute()
                    .inset_left(p.render_x())
                    .inset_top(p.render_top(wh))
                    .width(p.render_width())
                    .height(p.display_height())
                    .z_index(p.z_order)
            } else {
                // Pane was removed — hide this view until dyn_stack removes it.
                s.display(floem::style::Display::None)
            }
        })
}

// ---------------------------------------------------------------------------
// Resize handle — invisible edge/corner zones for pane resizing
// ---------------------------------------------------------------------------

/// Creates a single invisible resize handle for a pane edge.
///
/// Each handle is an `Empty` view positioned absolutely at the edge of
/// the pane. When clicked, it sets `resizing` to `Some(ResizeInfo)`,
/// which the pointer-move handler in `paned.rs` picks up to apply
/// delta-based resizing.
///
/// ## Handle positions
///
/// - **Left/Right**: full height below the header, `RESIZE_HANDLE_WIDTH` wide.
///   Side handles start below `PANE_HEADER_HEIGHT` to avoid conflicting
///   with the header's drag grip.
/// - **Top**: full width between corners, `RESIZE_HANDLE_WIDTH` tall.
/// - **TopLeft/TopRight**: small squares (`CORNER_HANDLE_SIZE`) at the corners.
fn resize_handle(
    pane_id: usize,
    edge: ResizeEdge,
    resizing: RwSignal<Option<ResizeInfo>>,
) -> impl IntoView {
    // Map each edge to its cursor style and positioning function.
    let (cursor, style_fn): (CursorStyle, Box<dyn Fn(floem::style::Style) -> floem::style::Style>) = match edge {
        ResizeEdge::Left => (CursorStyle::ColResize, Box::new(|s: floem::style::Style| {
            s.absolute()
                .inset_left(0.0)
                .inset_top(PANE_HEADER_HEIGHT) // below header
                .inset_bottom(0.0)
                .width(RESIZE_HANDLE_WIDTH)
        })),
        ResizeEdge::Right => (CursorStyle::ColResize, Box::new(|s: floem::style::Style| {
            s.absolute()
                .inset_right(0.0)
                .inset_top(PANE_HEADER_HEIGHT) // below header
                .inset_bottom(0.0)
                .width(RESIZE_HANDLE_WIDTH)
        })),
        ResizeEdge::Top => (CursorStyle::RowResize, Box::new(|s: floem::style::Style| {
            s.absolute()
                .inset_top(0.0)
                .inset_left(CORNER_HANDLE_SIZE) // between corner handles
                .inset_right(CORNER_HANDLE_SIZE)
                .height(RESIZE_HANDLE_WIDTH)
        })),
        ResizeEdge::TopLeft => (CursorStyle::NwResize, Box::new(|s: floem::style::Style| {
            s.absolute()
                .inset_top(0.0)
                .inset_left(0.0)
                .width(CORNER_HANDLE_SIZE)
                .height(CORNER_HANDLE_SIZE)
        })),
        ResizeEdge::TopRight => (CursorStyle::NeResize, Box::new(|s: floem::style::Style| {
            s.absolute()
                .inset_top(0.0)
                .inset_right(0.0)
                .width(CORNER_HANDLE_SIZE)
                .height(CORNER_HANDLE_SIZE)
        })),
    };

    // `Empty::new()` — an invisible view that only provides a hit area.
    Empty::new()
        .style(move |s| style_fn(s).cursor(cursor))
        .on_event_stop(listener::PointerDown, move |_, _| {
            // Set up resize state; the pointer-move handler in paned.rs
            // will apply the actual resizing.
            resizing.set(Some(ResizeInfo {
                pane_id,
                edge,
                last_x: None,
                last_y: None,
            }));
        })
}
