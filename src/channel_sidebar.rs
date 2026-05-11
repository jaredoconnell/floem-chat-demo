//! Channel sidebar panel — scrollable list of channels for the active server.
//!
//! ## Floem concepts demonstrated
//!
//! ### `VirtualStack` (aka `virtual_stack` / `VirtualStack::full`)
//!
//! A virtualized list that only renders the items currently visible in
//! the viewport. Essential for performance with large lists. It takes:
//!
//! 1. **Data source** — a closure returning `impl VirtualVector<T>`.
//!    Re-evaluated on each reactive cycle; when the return changes,
//!    VirtualStack diffs by key and adds/removes/reorders items.
//! 2. **Key function** — `|item| -> Key` for stable identity across updates.
//! 3. **View function** — `|item| -> impl IntoView` that builds each row.
//!
//! VirtualStack also supports `.item_size_fn()` for explicit per-item
//! sizing (see `chat_area.rs` for an example).
//!
//! ### `.scroll()` and `.scroll_to_percent()`
//!
//! `.scroll()` wraps a view in a scroll container. Only the visible
//! portion is rendered (when combined with VirtualStack).
//! `.scroll_to_percent(|| 100.0)` scrolls to the bottom, re-firing
//! whenever a signal read inside the closure changes.
//!
//! ### Dependency injection via closures
//!
//! This panel receives `server_name` and `filtered_channels` as closures
//! rather than raw signals. This keeps the component decoupled from the
//! global state shape — it doesn't know about servers or the full channel
//! list, only its filtered view.

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
    // VirtualStack::full is a convenience that combines virtual_stack() with
    // auto-sizing. The data closure wraps the filtered Vec in VecData
    // (our VirtualVector adapter). Each time `active_server` changes,
    // `filtered_channels()` returns a new Vec and VirtualStack diffs by
    // channel id to update the list.
    let channel_list = VirtualStack::full(
        move || VecData(filtered_channels()),
        // Key function: channel id provides stable identity.
        // VirtualStack uses this to diff old vs new lists efficiently.
        |ch: &Channel| ch.id,
        // View factory: called once per channel to create the row view.
        // `move` captures `active_channel` and `on_select` (both Copy).
        move |ch: Channel| {
            let cid = ch.id;
            channel_item(
                ch.name,
                // Reactive is_active: re-evaluated whenever active_channel changes.
                move || active_channel.get() == cid,
                move || {
                    active_channel.set(cid);
                    on_select(cid);
                },
            )
        },
    )
    .style(|s| s.flex_col().width_full().padding_horiz(8.0))
    // `.scroll()` wraps the VirtualStack in a scrollable container.
    .scroll()
    // `.scroll_to_percent(|| 100.0)` pins scroll to the bottom.
    // This re-fires whenever the data changes, keeping the list scrolled
    // to show the latest channels.
    .scroll_to_percent(|| 100.0)
    .style(|s| s.flex_grow(1.0).width_full());

    // Compose: header on top, scrollable channel list below.
    // `Stack::vertical` is Floem's flexbox column container.
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
