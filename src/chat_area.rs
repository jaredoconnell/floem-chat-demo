//! Chat message area — scrollable message timeline + text input.
//!
//! ## Floem concepts demonstrated
//!
//! ### `dyn_stack` for the message list
//!
//! `dyn_stack(data_fn, key_fn, view_fn)` renders all items and diffs by key
//! on each reactive update. Unlike `VirtualStack`, it doesn't require
//! pre-computed item sizes, so message rows can size naturally based on
//! their content (variable-height messages, text wrapping, etc.).
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

use floem::context::LayoutChanged;
use floem::views::dyn_stack;
use floem::style::CustomStylable;
use floem::views::Scroll;

use crate::components::{header_bar, message_input, message_row};
use crate::data::Message;
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
    panel_height: RwSignal<f64>,
) -> (impl IntoView, impl IntoView) {
    // Precompute cozy-mode grouping: `show_header` is true when the author
    // differs from the previous message (first in a group gets avatar +
    // name + timestamp; continuations show only the content).
    // Consumes the Vec from messages() with into_iter() to move each
    // Message directly instead of cloning it.
    let display_messages = move || -> Vec<(Message, bool)> {
        let msgs = messages();
        let mut result = Vec::with_capacity(msgs.len());
        let mut prev_author: Option<String> = None;
        for msg in msgs {
            let show_header = prev_author.as_deref() != Some(&msg.author);
            prev_author = Some(msg.author.clone());
            result.push((msg, show_header));
        }
        result
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

    // `dyn_stack` renders all items and diffs by key, allowing each row
    // to size naturally based on its content (no fixed heights needed).
    let message_list = dyn_stack(
        move || display_messages(),
        |(msg, _): &(Message, bool)| msg.id,
        move |(msg, show_header): (Message, bool)| {
            message_row(msg.author, msg.content, msg.timestamp, show_header)
        },
    )
    .style(|s| s.flex_col().width_full().padding_bottom(16.0))
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

    let (input_editor, line_count) = message_input("Message this channel…", wrapped_send);
    // The TextEditor's inner editor-content view is the actual focus target.
    // editor_view_id is set during view construction and available immediately.
    let editor_view_id = input_editor.editor().editor_view_id;

    // Read the actual line height from the editor's styling
    // (set to theme::MESSAGE_FONT_SIZE via SimpleStyling).
    let line_h = f64::from(input_editor.editor().line_height(0));

    // Inner padding creates a text inset within the input box. Floem uses
    // border-box sizing, so padding reduces the content area — the height
    // must include it.
    let pad = 8.0;
    let vertical_pad = pad * 2.0;

    // The TextEditor grows freely to fit its content. An outer Scroll
    // handles overflow (capped at 40% of the chat area) and provides a
    // visible scrollbar. This avoids fighting with the TextEditor's
    // internal Scroll, which is hard to style from the outside.
    let input = input_editor.style(move |s| {
        let lines = line_count.get();
        let desired = (lines as f64) * line_h + vertical_pad;
        // width_full() is safe here because the TextEditor is inside our
        // Scroll (no margins on this view), so 100% = the Scroll's width.
        s.width_full()
            .height(desired)
            .min_height(line_h + vertical_pad)
            .padding(pad)
            .background(theme::INPUT_BG)
            .color(theme::TEXT_PRIMARY)
            .border_radius(0.0)
    });

    // Wrap in our own Scroll so we fully control scrollbar visibility
    // and styling. scroll_to_percent keeps the cursor area visible as
    // the user types past the 40% cap.
    let input = Scroll::new(input)
        .scroll_to_percent(move || {
            line_count.track();
            100.0
        })
        .custom_style(|s: floem::views::scroll::ScrollCustomStyle| {
            s.handle_background(theme::TEXT_MUTED)
                .handle_border_radius(4.0)
                .hide_bars(false)
                .show_bars_when_idle(true)
        });

    // The 40% height cap is computed from panel_height (tracked by the
    // caller via LayoutChanged on the parent Stack). max_height_pct
    // doesn't reliably constrain a Scroll in taffy's flex layout.
    let input = input
        .style(move |s| {
            let lines = line_count.get();
            let desired = (lines as f64) * line_h + vertical_pad;
            let max_h = panel_height.get() * 0.4;
            s.height(desired.min(max_h))
                .min_height(line_h + vertical_pad)
                .margin_horiz(16.0)
                .margin_top(12.0)
        });

    // Helper to focus the editor's inner content view (the actual text
    // editing surface). The editor_view_id signal holds the ViewId once
    // the view tree is built.
    let focus_editor = move || {
        if let Some(id) = editor_view_id.get_untracked() {
            id.request_focus();
        }
    };

    // Clicking anywhere in the message timeline focuses the text input
    // so the user can start typing immediately without clicking the input.
    // We track pointer-down position and only focus on pointer-up if the
    // pointer barely moved — a significant drag means the user was
    // selecting text in a label and we shouldn't steal focus.
    let down_pos: RwSignal<Option<(f64, f64)>> = RwSignal::new(None);
    let message_list = message_list
        .on_event_cont(listener::PointerDown, move |_, event| {
            let pos = event.state.logical_point();
            down_pos.set(Some((pos.x, pos.y)));
        })
        .on_event_cont(listener::PointerUp, move |_, event| {
            if let Some((dx, dy)) = down_pos.get_untracked() {
                let pos = event.state.logical_point();
                let dist_sq = (pos.x - dx).powi(2) + (pos.y - dy).powi(2);
                // Label selection starts after >2px drag; use a matching
                // threshold so a normal click focuses the input but a
                // text-selection drag does not.
                if dist_sq <= 4.0 {
                    focus_editor();
                }
            }
            down_pos.set(None);
        });

    // Focus the text input shortly after mount so the view is in the tree.
    // Without the delay, the view might not have a layout yet and focus
    // would silently fail.
    floem::action::exec_after(Duration::from_millis(50), move |_| {
        focus_editor();
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
            focus_editor();
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
    let panel_height = RwSignal::new(700.0f64);
    let (message_list, input) =
        chat_area_contents(channel_name, messages, on_send, focus_input, panel_height);

    Stack::vertical((
        header_bar(channel_name, "# "),
        message_list,
        input,
    ))
    .on_event_cont(LayoutChanged::listener(), move |_cx, change| {
        panel_height.set(change.new_box.height());
    })
    .style(|s| {
        s.flex_grow(1.0)
            .height_full()
            .background(theme::CHAT_BG)
    })
}
