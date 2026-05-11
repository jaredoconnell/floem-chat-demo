//! Reusable Floem widget components.
//!
//! ## Floem concepts demonstrated in this module
//!
//! - **Reactive style closures** — `.style(move |s| ...)` is called by Floem
//!   whenever a signal read inside it changes. This is how views update their
//!   appearance reactively without explicit "set state → re-render" calls.
//!
//! - **Hover styles** — `.hover(|s| ...)` applies additional styles when the
//!   pointer is over the element. Floem handles the hover state tracking
//!   automatically; you just describe the delta.
//!
//! - **CSS transitions** — `.transition(Property, Transition::ease_in_out(duration))`
//!   animates property changes smoothly. Here `Background` transitions are used
//!   on the icon circle for a Discord-like morph effect.
//!
//! - **Events** — `.on_event_stop(listener, callback)` attaches an event handler
//!   and stops propagation. `on_event_cont` does the same but lets the event
//!   continue propagating to parent views. The `listener::Click` and
//!   `listener::PointerDown` constants identify which event to listen for.
//!
//! - **`into_any()`** — erases the concrete view type to `AnyView`, needed when
//!   `if/else` branches return different view types. Floem views are statically
//!   typed, so both branches must return the same type.
//!
//! - **`Empty::new()`** — a zero-size invisible view used as a spacer or
//!   placeholder when conditional content should take up no space.

use floem::prelude::*;
use floem::style::{Background, CursorStyle, Transition};
use floem::unit::DurationUnitExt;
use floem::views::{ClipExt, Decorators, Empty, TextInput, TextInputEnter};

use crate::avatar::user_avatar;
use crate::theme;

// ---------------------------------------------------------------------------
// Icon circle — used for server icons, reusable for any badge-style widget
// ---------------------------------------------------------------------------

/// Renders a square container with a centered letter, transitioning its
/// `border_radius` between fully round and slightly rounded on hover.
///
/// ## How the Discord-style morph works
///
/// - Default state: `border_radius = half` → perfect circle
/// - Active/hovered state: `border_radius = 0.3 * size` → rounded square
/// - The `transition(Background, ...)` animates the background color
///   change over 150ms with ease-in-out easing.
///
/// ## Callback pattern
///
/// `is_active` and `on_click` are `impl Fn() + Copy` — they're closures
/// that capture `RwSignal`s (which are `Copy`). This pattern lets the
/// component be reactive without knowing what signals drive it.
pub fn icon_circle(
    letter: char,
    bg_color: Color,
    size: f64,
    is_active: impl Fn() -> bool + 'static + Copy,
    on_click: impl Fn() + 'static + Copy,
) -> impl IntoView {
    let half = size / 2.0;
    let active_radius = size * 0.3; // slightly rounded square when active/hovered

    Label::new(letter.to_string())
        .style(move |s| {
            // This closure is reactive: `is_active()` reads a signal,
            // so Floem re-evaluates the style whenever that signal changes.
            s.width(size)
                .min_width(size)
                .height(size)
                .min_height(size)
                .justify_center()
                .items_center()
                .font_size(size * 0.45)
                .color(theme::TEXT_PRIMARY)
                .background(bg_color)
                // Reactively switch radius: circle when inactive, rounded square when active.
                .border_radius(if is_active() { active_radius } else { half })
                // Animate background changes (e.g. when switching between
                // active/inactive). `Background` is a style property identifier.
                .transition(
                    Background,
                    Transition::ease_in_out(150.millis()),
                )
                .cursor(CursorStyle::Pointer)
                // Hover pseudo-state: always show rounded-square on hover.
                .hover(move |s| s.border_radius(active_radius))
        })
        // `on_event_stop` stops event propagation so parent views don't also
        // handle this click. The callback signature is (view, event_data).
        .on_event_stop(listener::Click, move |_, _| on_click())
}

// ---------------------------------------------------------------------------
// Channel item — a `# channel-name` row
// ---------------------------------------------------------------------------

/// A clickable channel row, showing "# channel-name" with active/hover states.
///
/// The `is_active` closure is read inside the style closure, making the
/// appearance update reactively when the active channel changes.
pub fn channel_item(
    name: String,
    is_active: impl Fn() -> bool + 'static + Copy,
    on_click: impl Fn() + 'static + Copy,
) -> impl IntoView {
    let display = format!("# {name}");
    Label::new(display)
        .style(move |s| {
            let active = is_active();
            s.width_full()
                .padding_left(12.0)
                .padding_vert(6.0)
                .font_size(15.0)
                .border_radius(4.0)
                .color(if active {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_MUTED
                })
                .background(if active {
                    theme::ACTIVE_BG
                } else {
                    Color::TRANSPARENT
                })
                .cursor(CursorStyle::Pointer)
                .hover(move |s| {
                    // Only show hover effect on inactive items — active items
                    // already have a highlight background.
                    if !active {
                        s.background(theme::HOVER_BG).color(theme::TEXT_PRIMARY)
                    } else {
                        s
                    }
                })
        })
        .on_event_stop(listener::Click, move |_, _| on_click())
}

// ---------------------------------------------------------------------------
// Header bar — reused at the top of both the channel sidebar and chat area
// ---------------------------------------------------------------------------

/// A bold title bar used as a section header.
///
/// ## `Label::derived` vs `Label::new`
///
/// - `Label::new(string)` creates a static label — the text never changes.
/// - `Label::derived(closure)` creates a reactive label — Floem re-calls the
///   closure whenever a signal read inside it changes, updating the displayed
///   text automatically. Here, the server/channel name updates when the user
///   switches servers or channels.
pub fn header_bar(
    title: impl Fn() -> String + 'static,
    prefix: &'static str,
) -> impl IntoView {
    // `Label::derived` takes a closure that returns the text to display.
    // It re-evaluates whenever signals inside `title()` change.
    Label::derived(move || format!("{prefix}{}", title()))
        .style(|s| {
            s.width_full()
                .height(48.0)
                .padding_left(16.0)
                .items_center()
                .font_size(16.0)
                .font_weight(floem::text::FontWeight::BOLD)
                .color(theme::TEXT_PRIMARY)
                .background(theme::PANE_HEADER_BG)
                .border_bottom(1.0)
                .border_color(theme::HEADER_BORDER)
        })
}

// ---------------------------------------------------------------------------
// Message row — a single chat message with optional cozy-mode header
// ---------------------------------------------------------------------------

/// Fixed heights so VirtualStack's `item_size_fn` can provide exact values,
/// avoiding the default `Assume(None)` estimation (which starts at 10px and
/// only measures a single item, breaking variable-height layouts).
///
/// These must match the heights set via `.height()` in `message_row`'s style.
/// VirtualStack uses these to calculate scroll positions and viewport bounds
/// without laying out every item.
pub const MSG_HEIGHT_HEADER: f64 = 54.0;
pub const MSG_HEIGHT_CONTINUATION: f64 = 22.0;

/// Renders one message. When `show_header` is true, the avatar, author name,
/// and timestamp are displayed (first message in a cozy group). Otherwise only
/// the content is shown, indented to align with grouped messages.
///
/// ## `into_any()` for conditional views
///
/// The `if show_header { ... } else { ... }` branches return different view
/// types (e.g. `user_avatar(...)` vs `Empty::new()`). Floem views are
/// statically typed, so we call `.into_any()` on each branch to erase the
/// concrete type to `AnyView`, allowing both branches to have the same type.
///
/// ## `.clip()` for fixed-height rows
///
/// Long messages may wrap to more lines than fit in the fixed row height.
/// `.clip()` prevents overflow text from bleeding into adjacent rows.
/// This is important for VirtualStack, which positions rows at exact offsets.
pub fn message_row(
    author: String,
    content: String,
    timestamp: String,
    show_header: bool,
) -> impl IntoView {
    let row_height = if show_header {
        MSG_HEIGHT_HEADER
    } else {
        MSG_HEIGHT_CONTINUATION
    };

    let avatar_col = if show_header {
        user_avatar(&author)
            .style(|s| s.margin_top(2.0))
            .into_any()
    } else {
        // Invisible spacer keeping the indent consistent with header rows.
        Empty::new()
            .style(|s| s.width(32.0).min_width(32.0).height(0.0))
            .into_any()
    };

    let header_row = if show_header {
        Stack::horizontal((
            Label::new(author)
                .style(|s| {
                    s.font_size(15.0)
                        .font_weight(floem::text::FontWeight::SEMI_BOLD)
                        .color(theme::TEXT_PRIMARY)
                        .margin_right(8.0)
                }),
            Label::new(timestamp)
                .style(|s| s.font_size(11.0).color(theme::TEXT_MUTED)),
        ))
        .into_any()
    } else {
        Empty::new().style(|s| s.height(0.0)).into_any()
    };

    let content_label = Label::new(content)
        .style(|s| s.font_size(14.0).color(theme::TEXT_PRIMARY).text_wrap().width_full());

    // min_width(0) lets the text column shrink below its content width
    // in the flex row, preventing long messages from expanding the pane.
    // Without this, a long message would force the entire row to be as
    // wide as the longest line, breaking the fixed-width layout.
    let text_col = Stack::vertical((header_row, content_label))
        .style(|s| s.min_width(0.0).flex_grow(1.0));

    Stack::horizontal((avatar_col, text_col))
        .style(move |s| {
            s.width_full()
                .height(row_height)
                .col_gap(12.0) // gap between avatar and text columns
                .padding_left(16.0)
                .padding_right(16.0)
                .items_start() // align avatar to top, not center
                .padding_top(4.0)
        })
        // Clip wrapped text that exceeds the fixed row height so it
        // doesn't bleed into adjacent rows in the virtual list.
        .clip()
        .style(move |s| s.width_full().height(row_height))
}

// ---------------------------------------------------------------------------
// Message input — text field with submit-on-Enter
// ---------------------------------------------------------------------------

/// Self-contained text input that manages its own buffer. Calls `on_submit`
/// with the typed text when Enter is pressed, then clears the buffer.
///
/// Returns the concrete ``TextInput`` so callers can inspect its ``ViewId``
/// (e.g. to programmatically focus it via `view_id.request_focus()`).
///
/// ## `TextInput` and `RwSignal<String>`
///
/// Floem's `TextInput` is bound to an `RwSignal<String>` — the signal
/// *is* the source of truth for the text content. The widget reads from
/// and writes to this signal as the user types. To clear the input after
/// submit, we simply `buffer.set(String::new())`.
///
/// ## `TextInputEnter::listener()`
///
/// This is a custom event type specific to `TextInput`. It fires when
/// the user presses Enter inside the text field. We use `on_event_stop`
/// so the Enter keypress doesn't propagate further.
pub fn message_input(
    placeholder: &'static str,
    on_submit: impl Fn(String) + 'static + Copy,
) -> TextInput {
    let buffer = RwSignal::new(String::new());

    TextInput::new(buffer)
        .placeholder(placeholder)
        .on_event_stop(TextInputEnter::listener(), move |_, _| {
            let text = buffer.get_untracked();
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                on_submit(trimmed);
                buffer.set(String::new());
            }
        })
        .style(|s| {
            s.width_full()
                .padding(12.0)
                .font_size(14.0)
                .background(theme::INPUT_BG)
                .color(theme::TEXT_PRIMARY)
                .border_radius(8.0)
                .border(0.0)
        })
}

// ---------------------------------------------------------------------------
// Mini server icon — small non-interactive badge for pane headers
// ---------------------------------------------------------------------------

/// A small colored circle with a centered letter, used to indicate
/// which server a chat pane belongs to in the paned mode headers.
/// Similar to `icon_circle` but smaller and non-interactive.
pub fn mini_server_icon(letter: char, color: Color) -> impl IntoView {
    Label::new(letter.to_string()).style(move |s| {
        s.width(18.0)
            .min_width(18.0)
            .height(18.0)
            .min_height(18.0)
            .border_radius(9.0) // half of 18 = circle
            .background(color)
            .color(Color::WHITE)
            .justify_center()
            .items_center()
            .font_size(10.0)
            .margin_right(6.0)
    })
}

// ---------------------------------------------------------------------------
// Pane header — left content + close button, used in paned mode
// ---------------------------------------------------------------------------

/// Header bar for an individual pane. The caller provides the left-side
/// content (e.g. a label, an icon + label, etc.) and a close callback.
///
/// ## Composability with `impl IntoView`
///
/// The `left_content` parameter accepts anything that implements `IntoView`,
/// so callers can pass a simple `Label`, a `Stack` of icons and labels,
/// or any other view composition. This is Floem's core composition pattern.
pub fn pane_header(
    left_content: impl IntoView + 'static,
    on_close: impl Fn() + 'static + Copy,
) -> impl IntoView {
    // Wrap left_content in a horizontal stack with flex_grow so it fills
    // available space. min_width(0) allows text to truncate rather than
    // pushing the close button off-screen.
    let left = Stack::horizontal((left_content,))
        .style(|s| s.flex_grow(1.0).items_center().min_width(0.0));

    let close_btn = Label::new("x")
        .style(|s| {
            s.font_size(14.0)
                .color(theme::TEXT_MUTED)
                .padding(4.0)
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .hover(|s| s.color(theme::TEXT_PRIMARY).background(theme::HOVER_BG))
        })
        .on_event_stop(listener::Click, move |_, _| on_close());

    Stack::horizontal((left, close_btn))
        .style(|s| {
            s.width_full()
                .height(36.0)
                .padding_left(12.0)
                .padding_right(8.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(theme::PANE_BORDER)
        })
}
