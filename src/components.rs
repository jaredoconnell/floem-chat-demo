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
            s.width(size)
                .min_width(size)
                .height(size)
                .min_height(size)
                .justify_center()
                .items_center()
                .font_size(size * 0.45)
                .color(theme::TEXT_PRIMARY)
                .background(bg_color)
                .border_radius(if is_active() { active_radius } else { half })
                .transition(
                    Background,
                    Transition::ease_in_out(150.millis()),
                )
                .cursor(CursorStyle::Pointer)
                .hover(move |s| s.border_radius(active_radius))
        })
        .on_event_stop(listener::Click, move |_, _| on_click())
}

// ---------------------------------------------------------------------------
// Pill indicator — the small white bar beside the active server icon
// ---------------------------------------------------------------------------

/// A small white rounded pill that appears to the left of the active server.
pub fn pill_indicator(is_active: impl Fn() -> bool + 'static + Copy) -> impl IntoView {
    Empty::new()
        .style(move |s| {
            let active = is_active();
            s.width(4.0)
                .height(if active { 36.0 } else { 8.0 })
                .border_radius(4.0)
                .background(if active {
                    Color::WHITE
                } else {
                    Color::TRANSPARENT
                })
                .margin_right(4.0)
        })
}

// ---------------------------------------------------------------------------
// Channel item — a `# channel-name` row
// ---------------------------------------------------------------------------

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

pub fn header_bar(
    title: impl Fn() -> String + 'static,
    prefix: &'static str,
) -> impl IntoView {
    Label::derived(move || format!("{prefix}{}", title()))
        .style(|s| {
            s.width_full()
                .height(48.0)
                .padding_left(16.0)
                .items_center()
                .font_size(16.0)
                .font_weight(floem::text::FontWeight::BOLD)
                .color(theme::TEXT_PRIMARY)
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
pub const MSG_HEIGHT_HEADER: f64 = 54.0;
pub const MSG_HEIGHT_CONTINUATION: f64 = 22.0;

/// Renders one message. When `show_header` is true, the avatar, author name,
/// and timestamp are displayed (first message in a cozy group). Otherwise only
/// the content is shown, indented to align with grouped messages.
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
        // Invisible spacer keeping the indent consistent
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
    let text_col = Stack::vertical((header_row, content_label))
        .style(|s| s.min_width(0.0).flex_grow(1.0));

    Stack::horizontal((avatar_col, text_col))
        .style(move |s| {
            s.width_full()
                .height(row_height)
                .col_gap(12.0)
                .padding_left(16.0)
                .padding_right(16.0)
                .items_start()
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
pub fn message_input(
    placeholder: &'static str,
    on_submit: impl Fn(String) + 'static + Copy,
) -> impl IntoView {
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
/// which server a chat pane belongs to.
pub fn mini_server_icon(letter: char, color: Color) -> impl IntoView {
    Label::new(letter.to_string()).style(move |s| {
        s.width(18.0)
            .min_width(18.0)
            .height(18.0)
            .min_height(18.0)
            .border_radius(9.0)
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
pub fn pane_header(
    left_content: impl IntoView + 'static,
    on_close: impl Fn() + 'static + Copy,
) -> impl IntoView {
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
                .background(theme::PANE_HEADER_BG)
                .border_bottom(1.0)
                .border_color(theme::PANE_BORDER)
        })
}
