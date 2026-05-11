use std::time::Duration;

use floem::prelude::*;

use crate::components::{
    header_bar, message_input, message_row, MSG_HEIGHT_CONTINUATION, MSG_HEIGHT_HEADER,
};
use crate::data::{Message, VecData};
use crate::theme;

/// Builds the scrollable message list and input bar.
///
/// Returned as a tuple so callers can place them in their own layout
/// alongside a header of their choosing. Clicking the message list
/// focuses the text input for immediate typing.
///
/// ``focus_input`` is a reactive signal — bump it to programmatically
/// focus the text input (e.g. on channel switch or pane selection).
pub fn chat_area_contents(
    channel_name: impl Fn() -> String + 'static + Copy,
    messages: impl Fn() -> Vec<Message> + 'static + Copy,
    on_send: impl Fn(String) + 'static + Copy,
    focus_input: RwSignal<u64>,
) -> (impl IntoView, impl IntoView) {
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
                message_row(msg.author, msg.content, msg.timestamp, show_header)
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
    .style(|s| s.flex_col().flex_basis(0).flex_grow(1.0).min_width(0.0).width_full());

    let input = message_input("Message this channel…", wrapped_send);
    let input_id = input.view_id();
    let input = input
        .style(|s| s.padding_horiz(16.0).padding_bottom(16.0).padding_top(12.0).width_full());

    // Clicking the timeline focuses the text input for immediate typing.
    let message_list = message_list
        .on_event_cont(listener::PointerDown, move |_, _| {
            input_id.request_focus();
        });

    // Focus the text input shortly after mount so the view is in the tree.
    floem::action::exec_after(Duration::from_millis(50), move |_| {
        input_id.request_focus();
    });

    // Reactive style that watches the focus trigger for subsequent bumps
    // (channel switch, pane selection, etc.).
    let first_run = RwSignal::new(true);
    let input = input.style(move |s| {
        focus_input.get();
        if first_run.get_untracked() {
            first_run.set(false);
        } else {
            input_id.request_focus();
        }
        s
    });

    (message_list, input)
}

/// Main chat panel: channel header, scrollable message timeline, and input bar.
///
/// Has zero knowledge of channels, servers, or the message store — it receives
/// a flat message list and a send callback.
pub fn chat_area_panel(
    channel_name: impl Fn() -> String + 'static + Copy,
    messages: impl Fn() -> Vec<Message> + 'static + Copy,
    on_send: impl Fn(String) + 'static + Copy,
    focus_input: RwSignal<u64>,
) -> impl IntoView {
    let (message_list, input) = chat_area_contents(channel_name, messages, on_send, focus_input);

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
