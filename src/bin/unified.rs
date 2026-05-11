use floem::prelude::*;
use floem::window::WindowConfig;

use floem_demo::channel_sidebar::channel_sidebar_panel;
use floem_demo::chat_area::chat_area_panel;
use floem_demo::data::{AppState, Message};
use floem_demo::server_list::server_list_panel;

fn app_view() -> impl IntoView {
    let state = AppState::with_sample_data();

    let servers = state.servers;
    let channels = state.channels;
    let messages = state.messages;
    let active_server = state.active_server;
    let active_channel = state.active_channel;
    let next_message_id = state.next_message_id;

    // --- Derived closures for the channel sidebar ---
    let server_name = move || {
        let sid = active_server.get();
        servers
            .get()
            .iter()
            .find(|s| s.id == sid)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    };

    let filtered_channels = move || {
        let sid = active_server.get();
        channels
            .get()
            .into_iter()
            .filter(move |c| c.server_id == sid)
            .collect::<Vec<_>>()
    };

    // --- Derived closures for the chat area ---
    let channel_name = move || {
        let cid = active_channel.get();
        channels
            .get()
            .iter()
            .find(|c| c.id == cid)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };

    let current_messages = move || {
        let cid = active_channel.get();
        messages
            .get()
            .get(&cid)
            .cloned()
            .unwrap_or_default()
    };

    let on_send = move |text: String| {
        let cid = active_channel.get_untracked();
        let mid = next_message_id.get_untracked();
        next_message_id.set(mid + 1);

        let msg = Message {
            id: mid,
            channel_id: cid,
            author: "You".into(),
            content: text,
            timestamp: "Just now".into(),
        };

        messages.update(|m| {
            m.entry(cid).or_default().push(msg);
        });
    };

    // Bumped on channel selection to focus the chat text input.
    let focus_input = RwSignal::new(0u64);

    // --- Compose the three panels ---
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
    .window_title(|| "Discord Demo".to_string())
}

fn main() {
    floem::Application::new()
        .window(
            |_| app_view(),
            Some(WindowConfig::default().size((1100., 700.))),
        )
        .run();
}
