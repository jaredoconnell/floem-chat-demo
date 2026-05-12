//! Server icon strip — vertical list of guild icons.
//!
//! ## Floem concepts demonstrated
//!
//! ### `.container()` with conditional right border
//!
//! Wraps a view in a `Container` — a generic single-child wrapper that can
//! have its own styles. The active server's container gets a BLURPLE
//! `border_right`, placing an accent bar between the icon and the channel
//! sidebar. Inactive icons have no border and are dimmed via opacity.
//!
//! ### VirtualStack for small lists
//!
//! Even for small lists (2 servers here), VirtualStack is fine — the
//! overhead is negligible and it provides consistent key-based diffing.

use floem::prelude::*;

use crate::components::icon_circle;
use crate::data::{Channel, Server, VecData};
use crate::theme;

/// Vertical strip of server icons.
///
/// Clicking a server sets `active_server` and resets `active_channel` to the
/// first channel belonging to that server. The active server gets a BLURPLE
/// accent bar on its right edge; inactive icons are dimmed.
/// ``on_select`` is called after the active channel changes (e.g. to focus
/// the chat input).
pub fn server_list_panel(
    servers: RwSignal<Vec<Server>>,
    active_server: RwSignal<usize>,
    channels: RwSignal<Vec<Channel>>,
    active_channel: RwSignal<usize>,
    on_select: impl Fn(usize) + 'static + Copy,
) -> impl IntoView {
    VirtualStack::full(
        // Data source: `servers.get()` subscribes to the signal, so if
        // servers were ever added/removed, the list would re-diff.
        move || VecData(servers.get()),
        |s: &Server| s.id,
        move |server: Server| {
            let sid = server.id;
            // This closure is reactive — `active_server.get()` subscribes
            // the style to the active_server signal.
            let is_active = move || active_server.get() == sid;

            icon_circle(
                server.icon_letter,
                server.color(),
                42.0,
                is_active,
                move || {
                    active_server.set(sid);
                    // Reset to first channel of this server so the chat area
                    // shows relevant content immediately.
                    let first = channels
                        .get()
                        .iter()
                        .find(|c| c.server_id == sid)
                        .map(|c| c.id);
                    if let Some(ch_id) = first {
                        active_channel.set(ch_id);
                        on_select(ch_id);
                    }
                },
            )
            .container()
            .style(move |s| {
                let active = is_active();
                s.justify_center()
                    .items_center()
                    .padding(8.0)
                    .margin_bottom(2.0)
                    // Active server gets a BLURPLE bar on its right edge,
                    // between the icon and the channel sidebar.
                    .border_right(if active { 2.0 } else { 0.0 })
                    .border_color(theme::BLURPLE)
            })
        },
    )
    .style(|s| s.flex_col().items_center())
    .scroll()
    .style(|s| {
        // 48px header + 8px breathing room so icons start below the sidebar header.
        // `flex_shrink(0.0)` prevents the server strip from shrinking when
        // the window is narrow — it should always be exactly 62px wide.
        s.width(62.0)
            .min_width(62.0)
            .flex_shrink(0.0)
            .height_full()
            .flex_col()
            .padding_top(56.0)
            .background(theme::SERVER_BAR_BG)
    })
}
