//! Chat message area — scrollable message timeline + text input.
//!
//! ## Floem concepts demonstrated
//!
//! ### VirtualStack with `item_size_fn`
//!
//! By default, VirtualStack uses `Assume(None)` for item sizes — it measures
//! one item and assumes all items are the same height (defaulting to 10px if
//! none are measured yet). This breaks badly for variable-height items.
//!
//! `.item_size_fn(|item| -> f64)` provides explicit per-item sizes, which
//! VirtualStack uses to compute scroll positions and determine which items
//! are visible without laying out every item. The sizes must match the
//! heights set in the view's style.
//!
//! ### `scroll_to_percent` with reactive triggers
//!
//! `.scroll_to_percent(closure)` scrolls when the closure re-fires.
//! Using `.track()` on a trigger signal lets us control *when* scrolling
//! happens — only on user-send and channel-switch, not on every data change.
//!
//! ### `ViewId` and programmatic focus
//!
//! Every Floem view has a `ViewId` (accessible via `.view_id()`). The ID
//! can be used to imperatively control the view, e.g.:
//! - `view_id.request_focus()` — moves keyboard focus to the view.
//! This is used to auto-focus the text input on mount, channel switch,
//! and when clicking the message list.
//!
//! ### `exec_after` — delayed one-shot execution
//!
//! `floem::action::exec_after(duration, callback)` runs a callback after a
//! delay. Used here to focus the text input 50ms after mount, ensuring the
//! view is fully in the tree before requesting focus.
//!
//! ### Reactive style for side effects (focus trick)
//!
//! Style closures in Floem are reactive — they re-run when signals inside
//! them change. By reading `focus_input` (a trigger signal) inside a style
//! closure, we piggyback side effects (calling `request_focus()`) onto
//! the reactive system. The `first_run` guard skips the initial evaluation.
//! This is a pragmatic pattern when Floem doesn't provide a dedicated
//! "on signal change" callback.

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
    // differs from the previous message. This is done in the data layer
    // rather than the view layer so each VirtualStack item knows its height
    // at creation time (needed for `item_size_fn`).
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
    // This gives us precise control over when auto-scrolling happens.
    let scroll_trigger = RwSignal::new(0usize);

    let wrapped_send = move |text: String| {
        on_send(text);
        // Bump trigger to auto-scroll to the new message.
        scroll_trigger.update(|c| *c += 1);
    };

    let message_list = VirtualStack::full(
        move || VecData(display_messages()),
        // Key by message id for stable identity across list updates.
        |(msg, _): &(Message, bool)| msg.id,
        move |(msg, show_header): (Message, bool)| {
                message_row(msg.author, msg.content, msg.timestamp, show_header)
        },
    )
    // Explicit per-item sizes matching the fixed heights set on message_row.
    // Without this, VirtualStack uses Assume(None) which measures one item
    // and assumes all items are that height — completely wrong for our
    // variable-height (header vs continuation) rows.
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
        // Reading `scroll_trigger` subscribes this closure to the signal.
        // It re-fires on user-send (from wrapped_send above).
        scroll_trigger.track();
        // Reading `channel_name()` also subscribes, so switching channels
        // also triggers a scroll-to-bottom.
        let _ = channel_name();
        100.0 // scroll to 100% = bottom
    })
    .style(|s| {
        // `flex_basis(0)` + `flex_grow(1.0)` = fill remaining vertical space.
        // `min_width(0.0)` allows the column to shrink below content width
        // so long messages wrap instead of expanding the panel.
        s.flex_col().flex_basis(0).flex_grow(1.0).min_width(0.0).width_full()
    });

    let input = message_input("Message this channel…", wrapped_send);
    // Capture the ViewId before styling — we need it for focus management.
    let input_id = input.view_id();
    let input = input
        .style(|s| s.padding_horiz(16.0).padding_bottom(16.0).padding_top(12.0).width_full());

    // Clicking anywhere in the message timeline focuses the text input
    // so the user can start typing immediately without clicking the input.
    // `on_event_cont` (not `stop`) lets the click also select text etc.
    let message_list = message_list
        .on_event_cont(listener::PointerDown, move |_, _| {
            input_id.request_focus();
        });

    // Focus the text input shortly after mount so the view is in the tree.
    // Without the delay, the view might not have a layout yet and focus
    // would silently fail.
    floem::action::exec_after(Duration::from_millis(50), move |_| {
        input_id.request_focus();
    });

    // Reactive style that watches the focus trigger for subsequent bumps
    // (channel switch, pane selection, etc.).
    //
    // This is a Floem pattern for "do something when a signal changes":
    // read the signal inside a style closure to subscribe, then perform
    // the side effect. The `first_run` guard prevents the initial
    // evaluation from stealing focus prematurely.
    let first_run = RwSignal::new(true);
    let input = input.style(move |s| {
        // Reading focus_input subscribes this closure to the signal.
        focus_input.get();
        if first_run.get_untracked() {
            first_run.set(false);
        } else {
            // Subsequent fires = someone bumped focus_input, so focus.
            input_id.request_focus();
        }
        // Return the style unmodified — we only wanted the side effect.
        s
    });

    (message_list, input)
}

/// Main chat panel: channel header, scrollable message timeline, and input bar.
///
/// Has zero knowledge of channels, servers, or the message store — it receives
/// a flat message list and a send callback. This is the panel used by the
/// unified binary's three-column layout.
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
