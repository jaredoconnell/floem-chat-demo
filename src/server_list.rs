use floem::prelude::*;

use crate::components::{icon_circle, pill_indicator};
use crate::data::{Channel, Server, VecData};
use crate::theme;

/// Vertical strip of server icons.
///
/// Clicking a server sets `active_server` and resets `active_channel` to the
/// first channel belonging to that server.
pub fn server_list_panel(
    servers: RwSignal<Vec<Server>>,
    active_server: RwSignal<usize>,
    channels: RwSignal<Vec<Channel>>,
    active_channel: RwSignal<usize>,
) -> impl IntoView {
    VirtualStack::full(
        move || VecData(servers.get()),
        |s: &Server| s.id,
        move |server: Server| {
            let sid = server.id;
            let is_active = move || active_server.get() == sid;

            Stack::horizontal((
                pill_indicator(is_active),
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
                        }
                    },
                ),
            ))
            .style(|s| s.items_center().margin_bottom(4.0))
        },
    )
    .style(|s| s.flex_col())
    .scroll()
    .scroll_to_percent(|| 100.0)
    .style(|s| {
        s.width(56.0)
            .height_full()
            .flex_col()
            .items_center()
            .padding_top(8.0)
            .background(theme::SERVER_BAR_BG)
    })
}
