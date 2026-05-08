use floem::prelude::*;

use crate::avatar::user_avatar_svg;
use crate::components::{
    header_bar, message_input, message_row, MSG_HEIGHT_CONTINUATION, MSG_HEIGHT_HEADER,
};
use crate::data::{Message, VecData};
use crate::theme;

/// Main chat panel: channel header, scrollable message timeline, and input bar.
///
/// Has zero knowledge of channels, servers, or the message store — it receives
/// a flat message list and a send callback.
pub fn chat_area_panel(
    channel_name: impl Fn() -> String + 'static + Copy,
    messages: impl Fn() -> Vec<Message> + 'static + Copy,
    on_send: impl Fn(String) + 'static + Copy,
) -> impl IntoView {
    // Precompute cozy-mode grouping: `show_header` is true when the author
    // differs from the previous message.
    let display_messages = move || -> Vec<(Message, bool)> {
        let msgs = messages();
        msgs.iter()
            .enumerate()
            .map(|(i, msg)| {
                let show_header = i == 0 || msgs[i - 1].author != msg.author;
                (msg.clone(), show_header)
            })
            .collect()
    };

    // Incremented on user-send and channel-switch so scroll_to_percent
    // re-fires only for those events (not arbitrary data changes).
    let scroll_trigger = RwSignal::new(0usize);

    let wrapped_send = move |text: String| {
        on_send(text);
        scroll_trigger.update(|c| *c += 1);
    };

    let message_list = VirtualStack::full(
        move || VecData(display_messages()),
        |(msg, _): &(Message, bool)| msg.id,
        move |(msg, show_header): (Message, bool)| {
            let avatar = user_avatar_svg(&msg.author);
            message_row(msg.author, msg.content, msg.timestamp, show_header, avatar)
        },
    )
    // Explicit per-item sizes matching the fixed heights set on message_row,
    // so VirtualStack never falls back to the broken Assume(None) default.
    .item_size_fn(|(_, show_header): &(Message, bool)| {
        if *show_header {
            MSG_HEIGHT_HEADER
        } else {
            MSG_HEIGHT_CONTINUATION
        }
    })
    .style(|s| s.flex_col().width_full())
    .scroll()
    .scroll_to_percent(move || {
        // Re-fire on user-send or channel switch
        scroll_trigger.track();
        let _ = channel_name();
        100.0
    })
    .style(|s| s.flex_grow(1.0).width_full());

    let input = message_input("Message this channel…", wrapped_send)
        .style(|s| s.padding_horiz(16.0).padding_bottom(16.0).padding_top(0.0).width_full());

    Stack::vertical((
        header_bar(channel_name, "# "),
        message_list,
        input,
    ))
    .style(|s| {
        s.flex_grow(1.0)
            .height_full()
            .background(theme::CHAT_BG)
    })
}
