use floem::prelude::*;

use crate::components::icon_circle;
use crate::data::{Channel, Server, VecData};
use crate::theme;

/// Vertical strip of server icons.
///
/// Clicking a server sets `active_server` and resets `active_channel` to the
/// first channel belonging to that server. The active server row gets a
/// tab-shaped background that visually bridges into the channel sidebar.
/// ``on_select`` is called after the active channel changes (e.g. to
/// focus the chat input).
pub fn server_list_panel(
    servers: RwSignal<Vec<Server>>,
    active_server: RwSignal<usize>,
    channels: RwSignal<Vec<Channel>>,
    active_channel: RwSignal<usize>,
    on_select: impl Fn(usize) + 'static + Copy,
) -> impl IntoView {
    VirtualStack::full(
        move || VecData(servers.get()),
        |s: &Server| s.id,
        move |server: Server| {
            let sid = server.id;
            let is_active = move || active_server.get() == sid;

            // Tab-shaped row: active row bridges into the sidebar via matching background
            icon_circle(
                server.icon_letter,
                server.color(),
                42.0,
                is_active,
                move || {
                    active_server.set(sid);
                    // Reset to first channel of this server
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
                    .padding_left(10.0)
                    .padding_right(10.0)
                    .padding_vert(4.0)
                    .border_top_left_radius(8.0)
                    .border_bottom_left_radius(8.0)
                    .border_top_right_radius(0.0)
                    .border_bottom_right_radius(0.0)
                    .margin_bottom(2.0)
                    .background(if active {
                        theme::CHANNEL_SIDEBAR_BG
                    } else {
                        Color::TRANSPARENT
                    })
            })
        },
    )
    .style(|s| s.flex_col().items_end())
    .scroll()
    .style(|s| {
        // 48px header + 8px breathing room so icons start below the sidebar header
        s.width(62.0)
            .min_width(62.0)
            .flex_shrink(0.0)
            .height_full()
            .flex_col()
            .padding_top(56.0)
            .background(theme::SERVER_BAR_BG)
    })
}
