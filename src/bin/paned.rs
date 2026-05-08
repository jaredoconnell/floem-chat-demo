use std::collections::HashMap;

use floem::action::{exec_after_animation_frame, set_cursor_hittest};
use floem::prelude::*;
use floem::style::CursorStyle;
use floem::views::{ClipExt, Decorators, Empty, dyn_stack};
use floem::window::WindowConfig;

use floem_demo::chat_area::chat_area_contents;
use floem_demo::components::{channel_item, icon_circle, mini_server_icon, pane_header};
use floem_demo::data::{AppState, Channel, Message, Server};
use floem_demo::theme;

const DEFAULT_PANE_WIDTH: f64 = 350.0;
const BROWSER_PANE_WIDTH: f64 = 240.0;
const DEFAULT_PANE_HEIGHT: f64 = 500.0;
const PANE_SPACING: f64 = 8.0;
const TOOLBAR_HEIGHT: f64 = 48.0;
const PANE_HEADER_HEIGHT: f64 = 36.0;
const UNDOCK_THRESHOLD: f64 = 40.0;
const WINDOW_HEIGHT: f64 = 700.0;
const RESIZE_HANDLE_WIDTH: f64 = 4.0;
const MIN_PANE_WIDTH: f64 = 120.0;
/// How much of a stacked pane's edge is visible in the card-stack.
const PEEK_WIDTH: f64 = 40.0;
/// Minimum px/frame the animation moves (prevents crawling to a halt).
const MIN_ANIM_SPEED: f64 = 8.0;
/// Proportion of remaining distance moved each frame (ease-out).
const ANIM_FACTOR: f64 = 0.20;
/// Below this distance, snap exactly to target.
const ANIM_SNAP: f64 = 0.5;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum PaneKind {
    /// Server/channel browser pane.
    Browser,
    /// Chat timeline for a specific channel.
    Chat { channel_id: usize },
}

impl PaneKind {
    fn channel_id(&self) -> Option<usize> {
        match self {
            PaneKind::Chat { channel_id } => Some(*channel_id),
            PaneKind::Browser => None,
        }
    }
}

/// Identifies an open pane with its layout state.
///
/// `target_x` is the "slot" position panes animate toward (right-aligned,
/// packed with spacing). `x` is the current rendered position and is
/// animated toward `target_x` each frame unless the pane is being dragged.
#[derive(Clone, Debug)]
struct PaneState {
    id: usize,
    kind: PaneKind,
    /// Current rendered horizontal position (animated toward `target_x`).
    x: f64,
    /// Desired horizontal slot position; other panes animate toward this.
    target_x: f64,
    width: f64,
    height: f64,
    docked: bool,
    /// Only meaningful when `docked == false`: top edge in window coords.
    y: f64,
    /// When true, only the header bar is visible.
    collapsed: bool,
    /// Stable insertion order; lower = further right (older panes stay right,
    /// new panes appear to the left).
    dock_order: usize,
    /// True when this pane is compressed into a peek strip at a window edge.
    stacked: bool,
    /// Computed z-index for rendering order. Stacked panes nearest the
    /// visible area get the highest values so they render on top.
    z_order: i32,
}

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

#[derive(Clone, Copy, Debug)]
struct DragInfo {
    pane_id: usize,
    last_pointer_x: Option<f64>,
    last_pointer_y: Option<f64>,
    /// True once the pointer has moved (distinguishes click from drag).
    moved: bool,
}

#[derive(Clone, Copy, Debug)]
enum ResizeEdge {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
struct ResizeInfo {
    pane_id: usize,
    edge: ResizeEdge,
    last_x: Option<f64>,
}

// ---------------------------------------------------------------------------
// Card-stack layout helpers
// ---------------------------------------------------------------------------

/// Pack docked panes from the right edge. When the total width exceeds the
/// window, overflow panes compress into peek strips at the left/right edges
/// like a fanned deck of cards. `focus_id` determines which pane stays
/// fully visible (the most recently opened or interacted-with pane).
fn recompute_dock_targets(
    panes: &mut [PaneState],
    window_width: f64,
    focus_id: Option<usize>,
) {
    // Collect indices of docked panes sorted by dock_order (ascending).
    // Lower dock_order = further right (older panes stay on the right).
    let mut docked: Vec<usize> = panes
        .iter()
        .enumerate()
        .filter(|(_, p)| p.docked)
        .map(|(i, _)| i)
        .collect();
    docked.sort_by_key(|&i| panes[i].dock_order);

    let n = docked.len();
    if n == 0 {
        return;
    }

    // Reset stacked/z_order for all docked panes
    for &i in &docked {
        panes[i].stacked = false;
        panes[i].z_order = 0;
    }

    // Total width if every pane is shown at full size
    let total: f64 =
        docked.iter().map(|&i| panes[i].width).sum::<f64>() + (n as f64) * PANE_SPACING;

    if total <= window_width {
        // Everything fits — simple right-packing, lowest dock_order first
        // (rightmost position).
        let mut cursor = window_width;
        for &i in &docked {
            cursor -= panes[i].width + PANE_SPACING;
            panes[i].target_x = cursor;
        }
        return;
    }

    // --- Overflow: card-stack layout ---
    assign_card_stack_positions(&docked, panes, window_width, focus_id);
}

/// Core card-stack positioning. Determines which panes are fully visible and
/// which are compressed into peek strips at the edges.
///
/// `docked` must be sorted by dock_order ascending. Lower dock_order = further
/// right in the window. Overflow panes with lower dock_order than the visible
/// range stack on the right edge; higher dock_order overflow stacks on the left.
fn assign_card_stack_positions(
    docked: &[usize],
    panes: &mut [PaneState],
    window_width: f64,
    focus_id: Option<usize>,
) {
    let n = docked.len();

    // Find the focus pane in the sorted order; default to the highest
    // dock_order (newest, leftmost) if not found.
    let focus_sorted_idx = focus_id
        .and_then(|fid| docked.iter().position(|&i| panes[i].id == fid))
        .unwrap_or(n - 1);

    // Expand the visible range outward from the focus pane, preferring
    // rightward expansion first to keep the right-aligned feel.
    let mut vis_start = focus_sorted_idx;
    let mut vis_end = focus_sorted_idx;
    let mut used_width = panes[docked[focus_sorted_idx]].width;

    loop {
        let mut expanded = false;

        // Try expanding right (lower dock_order direction — toward the
        // right edge of the window, i.e. toward index 0 in sorted order).
        if vis_start > 0 {
            let candidate_w = panes[docked[vis_start - 1]].width + PANE_SPACING;
            let left_count = vis_start - 1;
            let right_count = n - vis_end - 1;
            let available =
                window_width - left_count as f64 * PEEK_WIDTH - right_count as f64 * PEEK_WIDTH;
            if used_width + candidate_w <= available {
                vis_start -= 1;
                used_width += candidate_w;
                expanded = true;
            }
        }

        // Try expanding left (higher dock_order — toward the left edge).
        if vis_end + 1 < n {
            let candidate_w = panes[docked[vis_end + 1]].width + PANE_SPACING;
            let left_count = vis_start;
            let right_count = n - vis_end - 2;
            let available =
                window_width - left_count as f64 * PEEK_WIDTH - right_count as f64 * PEEK_WIDTH;
            if used_width + candidate_w <= available {
                vis_end += 1;
                used_width += candidate_w;
                expanded = true;
            }
        }

        if !expanded {
            break;
        }
    }

    // docked[..vis_start] = lower dock_order = rightward in window → RIGHT stack
    // docked[vis_end+1..] = higher dock_order = leftward in window → LEFT stack
    let right_count = vis_start;
    let left_count = n - vis_end - 1;
    let right_peek_total = right_count as f64 * PEEK_WIDTH;
    let left_peek_total = left_count as f64 * PEEK_WIDTH;

    // --- Position visible panes: pack from right within the available space ---
    // Visible panes render above stacked peek strips (stacked z tops out
    // around 100 + stack_count, so 200 is safely above).
    let mut cursor = window_width - right_peek_total;
    for &i in &docked[vis_start..=vis_end] {
        cursor -= panes[i].width + PANE_SPACING;
        panes[i].target_x = cursor;
        panes[i].stacked = false;
        panes[i].z_order = 200;
    }
    if cursor < left_peek_total {
        let shift = left_peek_total - cursor;
        for &i in &docked[vis_start..=vis_end] {
            panes[i].target_x += shift;
        }
    }

    // --- Position right stack (docked[..vis_start], right edge of window) ---
    // Position by right edge so the RIGHT side of each pane peeks in.
    // k=0 is outermost (right edge flush with window), k=right_count-1 is
    // nearest to visible (right edge inset). Nearest renders on top.
    for (k, &sorted_idx) in docked[..vis_start].iter().enumerate() {
        panes[sorted_idx].target_x =
            window_width - k as f64 * PEEK_WIDTH - panes[sorted_idx].width;
        panes[sorted_idx].stacked = true;
        panes[sorted_idx].z_order = (k + 1) as i32 + 100;
    }

    // --- Position left stack (docked[vis_end+1..], left edge of window) ---
    // k=0 is nearest to visible, k=left_count-1 is outermost (highest
    // dock_order, leftmost). Nearest renders on top.
    for (k, &sorted_idx) in docked[vis_end + 1..].iter().enumerate() {
        panes[sorted_idx].target_x =
            (left_count - 1 - k) as f64 * PEEK_WIDTH;
        panes[sorted_idx].stacked = true;
        panes[sorted_idx].z_order = (left_count - k) as i32 + 100;
    }
}

/// While dragging (or resizing), figure out where the active pane should
/// slot in among the other docked panes, then re-lay-out using the
/// card-stack algorithm with the dragged pane as the focus.
fn recompute_targets_during_drag(
    panes: &mut [PaneState],
    drag_id: usize,
    window_width: f64,
) {
    let drag_idx = panes.iter().position(|p| p.id == drag_id);
    let Some(drag_idx) = drag_idx else { return };

    if !panes[drag_idx].docked {
        // Dragged pane is floating — just re-layout the remaining docked
        // panes using the last known focus (pick newest docked as fallback).
        let fallback = panes
            .iter()
            .filter(|p| p.docked && p.id != drag_id)
            .max_by_key(|p| p.dock_order)
            .map(|p| p.id);
        recompute_dock_targets(panes, window_width, fallback);
        return;
    }

    // Collect docked panes excluding the dragged one, sorted by dock_order
    let mut others: Vec<usize> = panes
        .iter()
        .enumerate()
        .filter(|(_, p)| p.docked && p.id != drag_id)
        .map(|(i, _)| i)
        .collect();
    others.sort_by_key(|&i| panes[i].dock_order);

    if others.is_empty() {
        return;
    }

    // Find where the dragged pane slots in among the others based on its
    // current x center. Lower indices = rightmost (lower dock_order).
    let drag_center = panes[drag_idx].x + panes[drag_idx].width / 2.0;
    let insert_pos = others
        .iter()
        .position(|&i| {
            let other_center = panes[i].target_x + panes[i].width / 2.0;
            drag_center > other_center
        })
        .unwrap_or(others.len());

    // Build the full ordered list with the dragged pane inserted
    let mut ordered: Vec<usize> = Vec::with_capacity(others.len() + 1);
    ordered.extend_from_slice(&others[..insert_pos]);
    ordered.push(drag_idx);
    ordered.extend_from_slice(&others[insert_pos..]);

    // Use the card-stack algorithm with the dragged pane as focus
    assign_card_stack_positions(&ordered, panes, window_width, Some(drag_id));
    // The dragged pane's position is mouse-controlled, not layout-controlled
    panes[drag_idx].target_x = panes[drag_idx].x;
}

/// After a drag ends, commit the dragged pane's spatial position into the
/// dock_order values so that subsequent re-layouts preserve the new ordering.
fn commit_drag_order(panes: &mut [PaneState], drag_id: usize) {
    let drag_idx = panes.iter().position(|p| p.id == drag_id);
    let Some(drag_idx) = drag_idx else { return };
    if !panes[drag_idx].docked {
        return;
    }

    let mut others: Vec<usize> = panes
        .iter()
        .enumerate()
        .filter(|(_, p)| p.docked && p.id != drag_id)
        .map(|(i, _)| i)
        .collect();
    others.sort_by_key(|&i| panes[i].dock_order);

    // Same spatial-insert logic as recompute_targets_during_drag
    let drag_center = panes[drag_idx].x + panes[drag_idx].width / 2.0;
    let insert_pos = others
        .iter()
        .position(|&i| {
            let other_center = panes[i].target_x + panes[i].width / 2.0;
            drag_center > other_center
        })
        .unwrap_or(others.len());

    let mut ordered: Vec<usize> = Vec::with_capacity(others.len() + 1);
    ordered.extend_from_slice(&others[..insert_pos]);
    ordered.push(drag_idx);
    ordered.extend_from_slice(&others[insert_pos..]);

    // Reassign contiguous dock_order values to match the new spatial order
    for (rank, &idx) in ordered.iter().enumerate() {
        panes[idx].dock_order = rank;
    }
}

// ---------------------------------------------------------------------------
// Animation tick
// ---------------------------------------------------------------------------

/// Advance each non-dragged pane's `x` toward its `target_x`.
/// Returns true if any pane still needs more animation.
fn tick_animation(panes: &mut [PaneState], drag_id: Option<usize>) -> bool {
    let mut needs_more = false;
    for pane in panes.iter_mut() {
        if Some(pane.id) == drag_id {
            continue;
        }
        let diff = pane.target_x - pane.x;
        if diff.abs() < ANIM_SNAP {
            pane.x = pane.target_x;
        } else {
            let step = diff * ANIM_FACTOR;
            // Enforce minimum speed so animation doesn't crawl
            let step = if step.abs() < MIN_ANIM_SPEED {
                MIN_ANIM_SPEED.copysign(diff)
            } else {
                step
            };
            // Clamp so the step never overshoots the target
            let step = if step.abs() > diff.abs() { diff } else { step };
            pane.x += step;
            needs_more = true;
        }
    }
    needs_more
}

/// Kick off the animation loop if one isn't already running.
/// `animating` prevents multiple concurrent loops from fighting.
fn start_animation(
    panes: RwSignal<Vec<PaneState>>,
    dragging: RwSignal<Option<DragInfo>>,
    animating: RwSignal<bool>,
) {
    if animating.get_untracked() {
        return;
    }
    animating.set(true);
    schedule_frame(panes, dragging, animating);
}

fn schedule_frame(
    panes: RwSignal<Vec<PaneState>>,
    dragging: RwSignal<Option<DragInfo>>,
    animating: RwSignal<bool>,
) {
    exec_after_animation_frame(move |_| {
        let drag_id = dragging.get_untracked().map(|d| d.pane_id);
        let needs_more = panes.try_update(|p| tick_animation(p, drag_id)).unwrap_or(false);
        if needs_more {
            schedule_frame(panes, dragging, animating);
        } else {
            animating.set(false);
        }
    });
}

// ---------------------------------------------------------------------------
// Browser pane content — server icons (left) + channel list (right)
// ---------------------------------------------------------------------------

/// Builds the interior of the server/channel browser pane: a narrow vertical
/// strip of server icons on the left, and a scrollable channel list for the
/// active server on the right. Clicking a channel calls
/// `on_open_channel(channel_id)`.
fn browser_content(
    servers: RwSignal<Vec<Server>>,
    channels: RwSignal<Vec<Channel>>,
    active_server: RwSignal<usize>,
    panes: RwSignal<Vec<PaneState>>,
    on_open_channel: impl Fn(usize) + 'static + Copy,
) -> impl IntoView {
    let server_col = dyn_stack(
        move || servers.get(),
        |s: &Server| s.id,
        move |server: Server| {
            let sid = server.id;
            icon_circle(
                server.icon_letter,
                server.color(),
                28.0,
                move || active_server.get() == sid,
                move || active_server.set(sid),
            )
        },
    )
    .style(|s| s.flex_col().row_gap(4.0).padding(4.0).items_center())
    .scroll()
    .style(|s| {
        s.width(40.0)
            .height_full()
            .background(theme::SERVER_BAR_BG)
            .border_right(1.0)
            .border_color(theme::PANE_BORDER)
    });

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
            // Highlight channels that already have an open pane
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

    Stack::horizontal((server_col, channel_list))
        .style(|s| s.width_full().flex_grow(1.0))
}

// ---------------------------------------------------------------------------
// Toolbar
// ---------------------------------------------------------------------------

fn toolbar(
    panes: RwSignal<Vec<PaneState>>,
    next_pane_id: RwSignal<usize>,
    window_size: RwSignal<(f64, f64)>,
    dragging: RwSignal<Option<DragInfo>>,
    animating: RwSignal<bool>,
    focus_pane_id: RwSignal<Option<usize>>,
) -> impl IntoView {
    let has_browser = move || {
        panes
            .get()
            .iter()
            .any(|p| matches!(p.kind, PaneKind::Browser))
    };

    // Only visible when the browser pane is closed
    let show_servers_btn = Label::new("☰ Servers")
        .style(move |s| {
            let vis = !has_browser();
            s.padding_horiz(12.0)
                .padding_vert(6.0)
                .font_size(14.0)
                .color(theme::TEXT_PRIMARY)
                .background(theme::BLURPLE)
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .hover(|s| s.background(Color::from_rgb8(100, 120, 200)))
                .display(if vis {
                    floem::style::Display::Flex
                } else {
                    floem::style::Display::None
                })
        })
        .on_event_stop(listener::Click, move |_, _| {
            let pid = next_pane_id.get_untracked();
            next_pane_id.set(pid + 1);
            let (ww, wh) = window_size.get_untracked();
            focus_pane_id.set(Some(pid));
            panes.update(|p| {
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
                    dock_order: pid,
                    stacked: false,
                    z_order: 0,
                });
                recompute_dock_targets(p, ww, Some(pid));
            });
            start_animation(panes, dragging, animating);
        });

    let drag_grip = Label::new("⠿")
        .style(|s| {
            s.font_size(18.0)
                .color(theme::TEXT_MUTED)
                .padding_horiz(8.0)
                .cursor(CursorStyle::Grab)
                .hover(|s| s.color(theme::TEXT_PRIMARY))
        })
        .on_event_stop(listener::PointerDown, |_, _| {
            floem::action::drag_window();
        });

    Stack::horizontal((drag_grip, show_servers_btn))
        .style(|s| {
            s.width_full()
                .height(TOOLBAR_HEIGHT)
                .padding_horiz(12.0)
                .col_gap(8.0)
                .items_center()
                .background(theme::PANE_HEADER_BG)
                .border_radius(8.0)
        })
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

fn pane_card(
    pane_id: usize,
    kind: PaneKind,
    servers: RwSignal<Vec<Server>>,
    channels: RwSignal<Vec<Channel>>,
    messages: RwSignal<HashMap<usize, Vec<Message>>>,
    next_message_id: RwSignal<usize>,
    panes: RwSignal<Vec<PaneState>>,
    next_pane_id: RwSignal<usize>,
    active_server: RwSignal<usize>,
    dragging: RwSignal<Option<DragInfo>>,
    resizing: RwSignal<Option<ResizeInfo>>,
    window_size: RwSignal<(f64, f64)>,
    animating: RwSignal<bool>,
    focus_pane_id: RwSignal<Option<usize>>,
) -> impl IntoView {
    let channel_id_opt = kind.channel_id();

    let on_close = move || {
        let (ww, _) = window_size.get_untracked();
        // If the closed pane was the focus, pick the newest remaining pane.
        let mut fid = focus_pane_id.get_untracked();
        if fid == Some(pane_id) {
            fid = None; // recompute will fall back to newest
        }
        panes.update(|p| {
            p.retain(|ps| ps.id != pane_id);
            recompute_dock_targets(p, ww, fid);
        });
        start_animation(panes, dragging, animating);
    };

    let start_drag = move || {
        // Don't set focus here; focus is determined on PointerUp based on
        // whether the user dragged or clicked, and whether the pane is stacked.
        dragging.set(Some(DragInfo {
            pane_id,
            last_pointer_x: None,
            last_pointer_y: None,
            moved: false,
        }));
    };

    // Build header left-content depending on pane kind.
    // Browser pane: plain "Servers" label.
    // Chat pane: mini server icon + "# channel-name".
    let header_content = match channel_id_opt {
        None => Label::new("Servers")
            .style(|s| {
                s.font_size(14.0)
                    .font_weight(floem::text::FontWeight::BOLD)
                    .color(theme::TEXT_PRIMARY)
            })
            .into_any(),
        Some(cid) => {
            // Look up the channel's server so we can show its icon.
            let server_info: Option<(char, (u8, u8, u8))> = {
                let chs = channels.get_untracked();
                let svs = servers.get_untracked();
                chs.iter()
                    .find(|c| c.id == cid)
                    .and_then(|ch| svs.iter().find(|s| s.id == ch.server_id))
                    .map(|s| (s.icon_letter, s.color_rgb))
            };

            let channel_label = Label::derived(move || {
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
            });

            if let Some((letter, (r, g, b))) = server_info {
                let icon = mini_server_icon(letter, Color::from_rgb8(r, g, b));
                Stack::horizontal((icon, channel_label))
                    .style(|s| s.items_center())
                    .into_any()
            } else {
                channel_label.into_any()
            }
        }
    };

    let header = pane_header(header_content, on_close)
        .style(|s| s.cursor(CursorStyle::Grab))
        .on_event_stop(listener::PointerDown, move |_, _| {
            start_drag();
        });

    // Build the pane body based on kind
    let content = if let Some(cid) = channel_id_opt {
        // Chat pane
        let channel_name = move || {
            channels
                .get()
                .iter()
                .find(|c| c.id == cid)
                .map(|c| c.name.clone())
                .unwrap_or_default()
        };
        let current_messages = move || {
            messages
                .get()
                .get(&cid)
                .cloned()
                .unwrap_or_default()
        };
        let on_send = move |text: String| {
            let mid = next_message_id.get_untracked();
            next_message_id.set(mid + 1);
            let msg = Message {
                id: mid,
                channel_id: cid,
                author: "You".into(),
                content: text,
                timestamp: "Just now".into(),
            };
            messages.update(|m| {
                m.entry(cid).or_default().push(msg);
            });
        };
        let (message_list, input) = chat_area_contents(channel_name, current_messages, on_send);
        Stack::vertical((message_list, input))
            .style(|s| s.width_full().flex_grow(1.0))
            .into_any()
    } else {
        // Browser pane — clicking a channel opens a chat pane
        let on_open_channel = move |channel_id: usize| {
            let already = panes
                .get_untracked()
                .iter()
                .any(|p| p.kind.channel_id() == Some(channel_id));
            if already {
                return;
            }
            let pid = next_pane_id.get_untracked();
            next_pane_id.set(pid + 1);
            let (ww, wh) = window_size.get_untracked();
            // Keep focus on the current pane (the browser) rather than
            // stealing it for the newly opened chat pane.
            let fid = focus_pane_id.get_untracked();
            panes.update(|p| {
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
                    dock_order: pid,
                    stacked: false,
                    z_order: 0,
                });
                recompute_dock_targets(p, ww, fid);
            });
            start_animation(panes, dragging, animating);
        };
        browser_content(servers, channels, active_server, panes, on_open_channel).into_any()
    };

    // Use sidebar bg for browser pane, chat bg for chat panes
    let bg = if channel_id_opt.is_some() {
        theme::CHAT_BG
    } else {
        theme::CHANNEL_SIDEBAR_BG
    };

    let clipped_content = Stack::vertical((header, content))
        .style(|s| s.width_full().height_full())
        .clip()
        // The Clip wrapper needs explicit sizing because the parent
        // Stack::new defaults to Row direction where width isn't
        // automatically stretched to the container.
        .style(|s| s.width_full().height_full());

    // Resize handles sit below the header to avoid conflicting with
    // the drag grip and close button.
    let left_handle = Empty::new()
        .style(|s| {
            s.absolute()
                .inset_left(0.0)
                .inset_top(PANE_HEADER_HEIGHT)
                .inset_bottom(0.0)
                .width(RESIZE_HANDLE_WIDTH)
                .cursor(CursorStyle::ColResize)
        })
        .on_event_stop(listener::PointerDown, move |_, _| {
            resizing.set(Some(ResizeInfo {
                pane_id,
                edge: ResizeEdge::Left,
                last_x: None,
            }));
        });

    let right_handle = Empty::new()
        .style(|s| {
            s.absolute()
                .inset_right(0.0)
                .inset_top(PANE_HEADER_HEIGHT)
                .inset_bottom(0.0)
                .width(RESIZE_HANDLE_WIDTH)
                .cursor(CursorStyle::ColResize)
        })
        .on_event_stop(listener::PointerDown, move |_, _| {
            resizing.set(Some(ResizeInfo {
                pane_id,
                edge: ResizeEdge::Right,
                last_x: None,
            }));
        });

    Stack::new((clipped_content, left_handle, right_handle))
        .style(move |s| {
            let (_, wh) = window_size.get();
            let current = panes.get();
            if let Some(p) = current.iter().find(|p| p.id == pane_id) {
                let display_height = if p.collapsed {
                    PANE_HEADER_HEIGHT
                } else {
                    p.height
                };
                let top = if p.docked { wh - display_height } else { p.y };
                s.absolute()
                    .inset_left(p.x)
                    .inset_top(top)
                    .width(p.width)
                    .height(display_height)
                    .background(bg)
                    .border_radius(8.0)
                    .border(1.0)
                    .border_color(theme::PANE_BORDER)
                    .z_index(p.z_order)
            } else {
                s.display(floem::style::Display::None)
            }
        })
}

// ---------------------------------------------------------------------------
// App root
// ---------------------------------------------------------------------------

fn app_view() -> impl IntoView {
    let state = AppState::with_sample_data();

    let servers = state.servers;
    let channels = state.channels;
    let messages = state.messages;
    let active_server = state.active_server;
    let next_message_id = state.next_message_id;

    let initial_width = 1200.0;
    let window_size: RwSignal<(f64, f64)> = RwSignal::new((initial_width, WINDOW_HEIGHT));

    // Start with the browser pane docked to the right
    let initial_x = initial_width - BROWSER_PANE_WIDTH - PANE_SPACING;
    let panes: RwSignal<Vec<PaneState>> = RwSignal::new(vec![PaneState {
        id: 0,
        kind: PaneKind::Browser,
        x: initial_x,
        target_x: initial_x,
        width: BROWSER_PANE_WIDTH,
        height: DEFAULT_PANE_HEIGHT,
        docked: true,
        y: WINDOW_HEIGHT - DEFAULT_PANE_HEIGHT,
        collapsed: false,
        dock_order: 0,
        stacked: false,
        z_order: 0,
    }]);
    let next_pane_id = RwSignal::new(1usize);
    let dragging: RwSignal<Option<DragInfo>> = RwSignal::new(None);
    let resizing: RwSignal<Option<ResizeInfo>> = RwSignal::new(None);
    let animating: RwSignal<bool> = RwSignal::new(false);
    let focus_pane_id: RwSignal<Option<usize>> = RwSignal::new(Some(0));

    let toolbar = toolbar(panes, next_pane_id, window_size, dragging, animating, focus_pane_id);

    let pane_area = dyn_stack(
        move || panes.get(),
        |ps: &PaneState| ps.id,
        move |ps: PaneState| {
            pane_card(
                ps.id,
                ps.kind.clone(),
                servers,
                channels,
                messages,
                next_message_id,
                panes,
                next_pane_id,
                active_server,
                dragging,
                resizing,
                window_size,
                animating,
                focus_pane_id,
            )
        },
    )
    .style(|s| s.width_full().height_full());

    Stack::new((pane_area, toolbar))
        .style(|s| s.width_full().height_full())
        .on_event_cont(
            listener::WindowResized,
            move |_, size: &floem::kurbo::Size| {
                let old = window_size.get_untracked();
                window_size.set((size.width, size.height));
                if (old.0 - size.width).abs() > 1.0 {
                    let fid = focus_pane_id.get_untracked();
                    panes.update(|p| recompute_dock_targets(p, size.width, fid));
                    start_animation(panes, dragging, animating);
                }
            },
        )
        // Unified pointer-move: resize > drag > click-through
        .on_event_cont(listener::PointerMove, move |_, event| {
            let pos = event.current.logical_point();

            // --- Resize ---
            if let Some(mut rz) = resizing.get_untracked() {
                if let Some(lx) = rz.last_x {
                    let dx = pos.x - lx;
                    let (ww, _) = window_size.get_untracked();
                    panes.update(|p| {
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == rz.pane_id) {
                            match rz.edge {
                                ResizeEdge::Right => {
                                    pane.width = (pane.width + dx).max(MIN_PANE_WIDTH);
                                }
                                ResizeEdge::Left => {
                                    let new_w = (pane.width - dx).max(MIN_PANE_WIDTH);
                                    let delta = pane.width - new_w;
                                    pane.x += delta;
                                    pane.width = new_w;
                                }
                            }
                        }
                        // Re-pack others while keeping the resized pane in place
                        recompute_targets_during_drag(p, rz.pane_id, ww);
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == rz.pane_id) {
                            pane.target_x = pane.x;
                        }
                    });
                    start_animation(panes, dragging, animating);
                }
                rz.last_x = Some(pos.x);
                resizing.set(Some(rz));
                return;
            }

            // --- Drag ---
            if let Some(mut drag) = dragging.get_untracked() {
                if let (Some(lx), Some(ly)) = (drag.last_pointer_x, drag.last_pointer_y) {
                    let dx = pos.x - lx;
                    let dy = pos.y - ly;
                    drag.moved = true;
                    let (ww, wh) = window_size.get_untracked();
                    panes.update(|p| {
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == drag.pane_id) {
                            pane.x += dx;
                            pane.target_x = pane.x;

                            if pane.docked {
                                pane.y += dy;
                                let dock_y = wh - pane.height;
                                if dock_y - pane.y > UNDOCK_THRESHOLD {
                                    pane.docked = false;
                                } else {
                                    pane.y = pane.y.min(dock_y);
                                }
                            } else {
                                pane.y += dy;
                                let dock_y = wh - pane.height;
                                if pane.y >= dock_y - PANE_SPACING {
                                    pane.docked = true;
                                    pane.y = dock_y;
                                }
                            }
                        }
                        recompute_targets_during_drag(p, drag.pane_id, ww);
                    });
                    start_animation(panes, dragging, animating);
                }
                drag.last_pointer_x = Some(pos.x);
                drag.last_pointer_y = Some(pos.y);
                dragging.set(Some(drag));
                return;
            }

            // --- Click-through on transparent areas ---
            let (_, wh) = window_size.get_untracked();
            let over_toolbar = pos.y <= TOOLBAR_HEIGHT;
            let over_pane = panes.get_untracked().iter().any(|p| {
                let dh = if p.collapsed { PANE_HEADER_HEIGHT } else { p.height };
                let top = if p.docked { wh - dh } else { p.y };
                pos.x >= p.x
                    && pos.x <= p.x + p.width
                    && pos.y >= top
                    && pos.y <= top + dh
            });
            if !over_toolbar && !over_pane {
                set_cursor_hittest(false);
                floem::action::exec_after(
                    std::time::Duration::from_millis(50),
                    move |_| set_cursor_hittest(true),
                );
            }
        })
        // Pointer up: finalize resize, drag, or toggle collapsed
        .on_event_cont(listener::PointerUp, move |_, _| {
            // --- Resize end ---
            if resizing.get_untracked().is_some() {
                let (ww, _) = window_size.get_untracked();
                let fid = focus_pane_id.get_untracked();
                panes.update(|p| recompute_dock_targets(p, ww, fid));
                resizing.set(None);
                start_animation(panes, dragging, animating);
                return;
            }

            // --- Drag end ---
            if let Some(drag) = dragging.get_untracked() {
                if drag.moved {
                    let (ww, wh) = window_size.get_untracked();
                    focus_pane_id.set(Some(drag.pane_id));
                    panes.update(|p| {
                        if let Some(pane) = p.iter_mut().find(|ps| ps.id == drag.pane_id) {
                            if !pane.docked {
                                let dock_y = wh - pane.height;
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
                    // Click without movement: if the pane is stacked,
                    // scroll it into view; otherwise toggle collapse.
                    let is_stacked = panes
                        .get_untracked()
                        .iter()
                        .find(|p| p.id == drag.pane_id)
                        .is_some_and(|p| p.stacked);
                    if is_stacked {
                        focus_pane_id.set(Some(drag.pane_id));
                        let (ww, _) = window_size.get_untracked();
                        panes.update(|p| {
                            recompute_dock_targets(p, ww, Some(drag.pane_id));
                        });
                    } else {
                        panes.update(|p| {
                            if let Some(pane) =
                                p.iter_mut().find(|ps| ps.id == drag.pane_id)
                            {
                                pane.collapsed = !pane.collapsed;
                            }
                        });
                    }
                }
                dragging.set(None);
                start_animation(panes, dragging, animating);
            }
        })
        .window_title(|| "Paned Demo".to_string())
}

fn main() {
    floem::Application::new()
        .window(
            |_| app_view(),
            Some(
                WindowConfig::default()
                    .size((1200., WINDOW_HEIGHT))
                    .with_transparent(true)
                    .undecorated(true),
            ),
        )
        .run();
}
