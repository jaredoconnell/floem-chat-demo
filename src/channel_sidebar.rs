use floem::prelude::*;

use crate::components::{channel_item, header_bar};
use crate::data::{Channel, VecData};
use crate::theme;

/// Channel sidebar panel.
///
/// Receives derived closures for the server name and filtered channel list so
/// it has no knowledge of servers or the global channel store.
/// ``on_select`` is called after a channel is activated (e.g. to focus
/// the chat input).
pub fn channel_sidebar_panel(
    server_name: impl Fn() -> String + 'static,
    filtered_channels: impl Fn() -> Vec<Channel> + 'static + Copy,
    active_channel: RwSignal<usize>,
    on_select: impl Fn(usize) + 'static + Copy,
) -> impl IntoView {
    let channel_list = VirtualStack::full(
        move || VecData(filtered_channels()),
        |ch: &Channel| ch.id,
        move |ch: Channel| {
            let cid = ch.id;
            channel_item(
                ch.name,
                move || active_channel.get() == cid,
                move || {
                    active_channel.set(cid);
                    on_select(cid);
                },
            )
        },
    )
    .style(|s| s.flex_col().width_full().padding_horiz(8.0))
    .scroll()
    .scroll_to_percent(|| 100.0)
    .style(|s| s.flex_grow(1.0).width_full());

    Stack::vertical((
        header_bar(server_name, ""),
        channel_list,
    ))
    .style(|s| {
        s.width(240.0)
            .height_full()
            .background(theme::CHANNEL_SIDEBAR_BG)
    })
}
