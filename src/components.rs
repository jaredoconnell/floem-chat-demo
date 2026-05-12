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

use std::str::FromStr;

use floem::prelude::*;
use floem::style::{Background, CursorStyle, Transition};
use floem::unit::DurationUnitExt;
use floem::views::editor::command::CommandExecuted;
use floem::views::editor::core::buffer::rope_text::RopeText;
use floem::views::editor::core::cursor::{Cursor, CursorAffinity, CursorMode};
use floem::views::editor::core::editor::EditType;
use floem::views::editor::core::selection::Selection;
use floem::views::editor::keypress::KeypressMap;
use floem::views::editor::text::{SimpleStyling, WrapMethod};
use floem::views::{Decorators, Empty, TextEditor, text_editor_keys};
use floem::{Clipboard, Menu};

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
    let active_radius = size * 0.12; // subtly rounded square when active/hovered

    // Dimmed variants for inactive/hover states.
    // Floem's `opacity` style isn't wired into the vello renderer, so we
    // apply alpha directly to the background and text colors instead.
    let dim_bg = bg_color.with_alpha(0.55);
    let dim_text = theme::TEXT_PRIMARY.with_alpha(0.55);
    let hover_bg = bg_color.with_alpha(0.8);
    let hover_text = theme::TEXT_PRIMARY.with_alpha(0.8);

    Label::new(letter.to_string())
        .style(move |s| {
            let active = is_active();
            // This closure is reactive: `is_active()` reads a signal,
            // so Floem re-evaluates the style whenever that signal changes.
            s.width(size)
                .min_width(size)
                .height(size)
                .min_height(size)
                .justify_center()
                .items_center()
                .font_size(size * 0.45)
                .color(if active { theme::TEXT_PRIMARY } else { dim_text })
                .background(if active { bg_color } else { dim_bg })
                // Reactively switch radius: circle when inactive, rounded square when active.
                .border_radius(if active { active_radius } else { half })
                .scale(if active { 100.0 } else { 90.0 })
                // Animate background changes smoothly.
                .transition(
                    Background,
                    Transition::ease_in_out(150.millis()),
                )
                .cursor(CursorStyle::Pointer)
                // Hover only affects inactive icons — active stays at full brightness.
                .hover(move |s| {
                    if is_active() {
                        s
                    } else {
                        s.border_radius(active_radius)
                            .scale(100.0)
                            .background(hover_bg)
                            .color(hover_text)
                    }
                })
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
pub fn message_row(
    author: String,
    content: String,
    timestamp: String,
    show_header: bool,
) -> impl IntoView {

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

    // Floem only checks the directly-hit (deepest) view for a context
    // menu, so we attach it to the content label — the leaf the user
    // actually right-clicks on.
    let content_for_menu = content.clone();
    let content_label = Label::new(content)
        .style(|s| s.font_size(theme::MESSAGE_FONT_SIZE as f32).color(theme::TEXT_PRIMARY).text_wrap().width_full())
        .context_menu(move || {
            let content = content_for_menu.clone();
            Menu::new()
                .item("Copy Message", move |i| {
                    let content = content.clone();
                    i.action(move || {
                        let _ = Clipboard::set_contents(content.clone());
                    })
                })
        });

    // min_width(0) lets the text column shrink below its content width
    // in the flex row, preventing long messages from expanding the pane.
    // Without this, a long message would force the entire row to be as
    // wide as the longest line, breaking the fixed-width layout.
    let text_col = Stack::vertical((header_row, content_label))
        .style(|s| s.min_width(0.0).flex_grow(1.0));

    Stack::horizontal((avatar_col, text_col))
        .style(move |s| {
            s.width_full()
                .col_gap(12.0) // gap between avatar and text columns
                .padding_left(16.0)
                .padding_right(16.0)
                .items_start() // align avatar to top, not center
                .padding_top(4.0)
        })
}

// ---------------------------------------------------------------------------
// Message input — multi-line editor with submit-on-Enter
// ---------------------------------------------------------------------------

/// Multi-line text input for composing chat messages.
///
/// Returns a ``(TextEditor, RwSignal<usize>)`` tuple: the editor widget
/// and a reactive line-count signal (document lines, updated on each edit).
/// Callers use the line-count signal to dynamically size the input area.
///
/// ## Key behaviour
///
/// - **Enter** submits the message and clears the editor.
/// - **Shift+Enter** inserts a newline (standard multi-line editing).
/// - All other keys are handled by the default editor keymap.
///
/// ## `text_editor_keys` and custom key handling
///
/// `text_editor_keys(initial_text, handler)` builds a full ``TextEditor``
/// with a caller-supplied key handler. The handler receives the
/// ``RwSignal<Editor>`` and the ``KeypressKey`` and returns
/// ``CommandExecuted::Yes`` if it consumed the event. We intercept bare
/// Enter for submit and delegate everything else (including Shift+Enter)
/// to ``KeypressMap::default()``.
pub fn message_input(
    placeholder: &'static str,
    on_submit: impl Fn(String) + 'static + Copy,
) -> (TextEditor, RwSignal<usize>) {
    let line_count = RwSignal::new(1usize);
    let default_keymap = KeypressMap::default();
    let enter_key = Key::from_str("Enter").unwrap();

    let tab_key = Key::Named(NamedKey::Tab);

    let editor = text_editor_keys("", move |editor_sig, kp| {
        // Don't consume Tab — let it propagate for focus navigation.
        if kp.key == tab_key {
            return CommandExecuted::No;
        }

        let is_enter = kp.key == enter_key;
        let has_shift = kp.modifiers.contains(Modifiers::SHIFT);

        if is_enter && !has_shift {
            // Extract text, submit, and clear the editor.
            let text = editor_sig.with_untracked(|ed| {
                let rt = ed.rope_text();
                let len = rt.len();
                if len == 0 {
                    return String::new();
                }
                rt.slice_to_cow(0..len).to_string()
            });
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                on_submit(trimmed);
                editor_sig.with_untracked(|ed| {
                    let len = ed.rope_text().len();
                    if len > 0 {
                        let sel = Selection::region(0, len, CursorAffinity::Forward);
                        ed.doc()
                            .edit(&mut std::iter::once((sel, "")), EditType::DeleteSelection);
                    }
                    ed.cursor.set(Cursor {
                        mode: CursorMode::Insert(Selection::caret(0, CursorAffinity::Forward)),
                        horiz: None,
                        motion_mode: None,
                        history_selections: Vec::new(),
                    });
                });
                line_count.set(1);
            }
            CommandExecuted::Yes
        } else {
            // Shift+Enter inserts a newline via the default keymap;
            // all other keys are handled normally.
            default_keymap.handle_keypress(editor_sig, kp)
        }
    })
    .placeholder(placeholder)
    .styling(
        SimpleStyling::builder()
            .font_size(theme::MESSAGE_FONT_SIZE as usize)
            .build(),
    )
    .editor_style(|s| {
        s.hide_gutter(true)
            .wrap_method(WrapMethod::EditorWidth)
            .scroll_beyond_last_line(false)
            .cursor_color(theme::TEXT_PRIMARY)
    })
    // Track visual line count (including soft-wrapped lines) for dynamic height.
    // Force text layout creation so last_vline() accounts for wrapping.
    .update(move |on_update| {
        if let Some(ed) = on_update.editor {
            for line in 0..ed.num_lines() {
                ed.text_layout(line);
            }
            let visual_lines = ed.last_vline().get() + 1;
            line_count.set(visual_lines);
        }
    });

    (editor, line_count)
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
