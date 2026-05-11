//! Unified demo binary — classic three-column Discord-style layout.
//!
//! ## Floem application structure
//!
//! A Floem app follows this pattern:
//! 1. `main()` creates a `floem::Application`.
//! 2. `.window(view_fn, config)` opens a window. The `view_fn` closure
//!    returns the root view (must implement `IntoView`).
//! 3. `.run()` enters the event loop (blocks until the app quits).
//!
//! ## `WindowConfig`
//!
//! `WindowConfig::default().size((w, h))` sets the initial window size
//! in logical pixels. Other options include `.with_transparent(true)`,
//! `.undecorated(true)`, `.position(x, y)`, etc.
//!
//! ## Layout: `Stack::horizontal`
//!
//! The three-column layout uses `Stack::horizontal((left, center, right))`.
//! `Stack::horizontal` is Floem's flexbox row container (like CSS
//! `display: flex; flex-direction: row`). Children fill available space
//! based on their `width`, `min_width`, and `flex_grow` settings:
//!
//! - Server strip: fixed 62px (`width(62.0)`, `flex_shrink(0.0)`)
//! - Channel sidebar: fixed 240px (`width(240.0)`)
//! - Chat area: fills remainder (`flex_grow(1.0)`)
//!
//! ## Signal wiring pattern
//!
//! `AppState::with_sample_data()` creates all signals. Individual signals
//! are then destructured and passed to child panels. Each panel only
//! receives the signals it needs — this is Floem's "props-down, events-up"
//! pattern (similar to React props, but with reactive signals instead of
//! plain values).
//!
//! Derived closures (like `server_name`, `filtered_channels`) transform
//! raw signals into the shape each panel expects, keeping panels decoupled
//! from the global state structure.

use floem::prelude::*;
use floem::window::WindowConfig;

use floem_demo::channel_sidebar::channel_sidebar_panel;
use floem_demo::chat_area::chat_area_panel;
use floem_demo::data::AppState;
use floem_demo::server_list::server_list_panel;

/// Build the root view: server strip | channel sidebar | chat area.
///
/// This function creates the entire UI tree. Floem calls it once at
/// window creation; after that, the reactive system handles all updates
/// through signal subscriptions.
fn app_view() -> impl IntoView {
    // Create the central reactive state with sample data.
    // Each field is an independent RwSignal.
    let state = AppState::with_sample_data();

    // Destructure individual signals for passing to child panels.
    // Because RwSignal is Copy, these are cheap copies of signal IDs.
    let servers = state.servers;
    let channels = state.channels;
    let active_server = state.active_server;
    let active_channel = state.active_channel;

    // --- Derived closures for the channel sidebar ---
    // These closures read signals and transform the data into the shape
    // the sidebar expects. They're re-evaluated by Floem's reactive
    // system whenever the signals they read change.
    let server_name = move || {
        let sid = active_server.get();
        servers.with(|svs| {
            svs.iter()
                .find(|s| s.id == sid)
                .map(|s| s.name.clone())
                .unwrap_or_default()
        })
    };

    let filtered_channels = move || {
        let sid = active_server.get();
        channels.with(|chs| {
            chs.iter()
                .filter(|c| c.server_id == sid)
                .cloned()
                .collect::<Vec<_>>()
        })
    };

    // --- Derived closures for the chat area ---
    let channel_name = move || {
        let cid = active_channel.get();
        channels.with(|chs| {
            chs.iter()
                .find(|c| c.id == cid)
                .map(|c| c.name.clone())
                .unwrap_or_default()
        })
    };

    let current_messages = {
        let messages = state.messages;
        move || {
            let cid = active_channel.get();
            // Borrow the HashMap instead of cloning it; only clone
            // the Vec for the active channel.
            messages.with(|m| m.get(&cid).cloned().unwrap_or_default())
        }
    };

    // Capture only the signals needed for sending, not the full AppState.
    // This avoids the `AppState: Copy` issue — AppState contains non-Copy
    // fields, but individual RwSignals are Copy.
    let messages = state.messages;
    let next_message_id = state.next_message_id;
    let on_send = move |text: String| {
        let cid = active_channel.get_untracked();
        floem_demo::data::send_message(messages, next_message_id, cid, text);
    };

    // Bumped on channel selection to focus the chat text input.
    let focus_input = RwSignal::new(0u64);

    // --- Compose the three panels ---
    // `Stack::horizontal` arranges children left-to-right in a flexbox row.
    Stack::horizontal((
        server_list_panel(servers, active_server, channels, active_channel, move |_| {
            focus_input.update(|v| *v += 1);
        }),
        channel_sidebar_panel(server_name, filtered_channels, active_channel, move |_| {
            focus_input.update(|v| *v += 1);
        }),
        chat_area_panel(channel_name, current_messages, on_send, focus_input),
    ))
    .style(|s| s.width_full().height_full())
    // `.window_title` sets the OS window title bar text.
    .window_title(|| "Discord Demo".to_string())
}

fn main() {
    // Create the Floem application, open one window, and enter the event loop.
    floem::Application::new()
        .window(
            |_| app_view(),
            Some(WindowConfig::default().size((1100., 700.))),
        )
        .run();
}
