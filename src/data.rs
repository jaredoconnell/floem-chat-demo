use std::collections::HashMap;
use std::ops::Range;

use floem::prelude::*;

/// Newtype wrapper so `Vec<T>` can be used as a `VirtualVector` data source.
/// Floem only implements `VirtualVector` for `RwSignal<Vec<T>>`, `ReadSignal<Vec<T>>`,
/// ranges, and `imbl::Vector` — not plain `Vec`.
#[derive(Clone)]
pub struct VecData<T>(pub Vec<T>);

impl<T: Clone> VirtualVector<T> for VecData<T> {
    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(&self, range: Range<usize>) -> impl Iterator<Item = T> {
        self.0[range].iter().cloned()
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Server {
    pub id: usize,
    pub name: String,
    pub icon_letter: char,
    /// Stored as `(r, g, b)` to keep this struct `Hash + Eq`.
    pub color_rgb: (u8, u8, u8),
}

impl Server {
    pub fn color(&self) -> Color {
        let (r, g, b) = self.color_rgb;
        Color::from_rgb8(r, g, b)
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Channel {
    pub id: usize,
    pub server_id: usize,
    pub name: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Message {
    pub id: usize,
    pub channel_id: usize,
    pub author: String,
    pub content: String,
    pub timestamp: String,
}

/// Central reactive state. Created once in `main.rs`, then individual signals
/// are passed to whichever panel needs them — the struct itself is never shared.
pub struct AppState {
    pub servers: RwSignal<Vec<Server>>,
    pub channels: RwSignal<Vec<Channel>>,
    pub messages: RwSignal<HashMap<usize, Vec<Message>>>,
    pub active_server: RwSignal<usize>,
    pub active_channel: RwSignal<usize>,
    pub next_message_id: RwSignal<usize>,
}

impl AppState {
    pub fn with_sample_data() -> Self {
        let (servers, channels, messages, next_id) = build_sample_data();
        AppState {
            servers: RwSignal::new(servers),
            channels: RwSignal::new(channels),
            messages: RwSignal::new(messages),
            active_server: RwSignal::new(0),
            active_channel: RwSignal::new(0),
            next_message_id: RwSignal::new(next_id),
        }
    }
}

const AUTHORS: &[&str] = &["Ferris", "Alice", "Bob", "Carol", "Dave", "Eve"];

fn build_sample_data() -> (
    Vec<Server>,
    Vec<Channel>,
    HashMap<usize, Vec<Message>>,
    usize,
) {
    let servers = vec![
        Server {
            id: 0,
            name: "Rust Hub".into(),
            icon_letter: 'R',
            color_rgb: (114, 137, 218), // blurple
        },
        Server {
            id: 1,
            name: "Game Dev".into(),
            icon_letter: 'G',
            color_rgb: (87, 242, 135), // green
        },
    ];

    let mut channel_id: usize = 0;
    let mut channels = Vec::new();
    let mut messages: HashMap<usize, Vec<Message>> = HashMap::new();
    let mut msg_id: usize = 0;

    // --- Server 0: Rust Hub (3 channels, ~10 messages each) ---
    for name in &["general", "help", "showcase"] {
        let ch = Channel {
            id: channel_id,
            server_id: 0,
            name: name.to_string(),
        };
        messages.insert(channel_id, make_messages(channel_id, 10, &mut msg_id));
        channels.push(ch);
        channel_id += 1;
    }

    // --- Server 1: Game Dev (8 channels) ---
    let gd_channels = [
        "general",
        "art",
        "programming",
        "audio",
        "level-design",
        "off-topic",
        "feedback",
        "announcements",
    ];
    for (i, name) in gd_channels.iter().enumerate() {
        let ch = Channel {
            id: channel_id,
            server_id: 1,
            name: name.to_string(),
        };
        // First channel ("general") gets 300 messages for stress testing
        let count = if i == 0 { 300 } else { 10 };
        messages.insert(channel_id, make_messages(channel_id, count, &mut msg_id));
        channels.push(ch);
        channel_id += 1;
    }

    (servers, channels, messages, msg_id)
}

fn make_messages(channel_id: usize, count: usize, next_id: &mut usize) -> Vec<Message> {
    let mut msgs = Vec::with_capacity(count);
    // Offset author start and content by channel_id so each channel
    // has visually distinct conversations.
    let mut author_idx = channel_id % AUTHORS.len();
    let mut run_remaining = 0;

    for i in 0..count {
        if run_remaining == 0 {
            author_idx = (author_idx + 1) % AUTHORS.len();
            // Runs of 1-3 messages from the same author
            run_remaining = (i % 3) + 1;
        }
        run_remaining -= 1;

        let hour = 9 + (i * 3) % 12;
        let minute = (i * 7) % 60;
        let period = if hour >= 12 { "PM" } else { "AM" };
        let display_hour = if hour > 12 { hour - 12 } else { hour };

        // Offset content index by channel so each channel gets different messages
        let content_offset = channel_id * 7;
        msgs.push(Message {
            id: *next_id,
            channel_id,
            author: AUTHORS[author_idx].to_string(),
            content: sample_message_text(i + content_offset),
            timestamp: format!("Today at {display_hour}:{minute:02} {period}"),
        });
        *next_id += 1;
    }
    msgs
}

fn sample_message_text(i: usize) -> String {
    let pool = [
        "Hey everyone, how's it going?",
        "Just pushed a new commit, check it out!",
        "Has anyone tried the new async runtime?",
        "I think we should refactor the event system. The current approach has too many layers of indirection and it makes debugging really painful when events get lost somewhere in the pipeline.",
        "Nice work on the PR!",
        "Can someone review my changes?",
        "The build is passing now.",
        "I'm stuck on a lifetime issue, any ideas? I have a struct that borrows from two different sources and the compiler keeps complaining about conflicting lifetimes. I tried adding explicit lifetime annotations but now the trait bounds don't work.",
        "Let's discuss the architecture for v2.",
        "Great meeting today!",
        "Working on the renderer module.",
        "Fixed that nasty race condition.",
        "Anyone up for a code review session?",
        "The benchmarks look promising — we're seeing a 40% improvement in throughput on the hot path after switching from Box<dyn Trait> to an enum dispatch. The cold path is slightly slower but that's an acceptable trade-off for our workload.",
        "I added some tests for the edge cases.",
        "Documentation needs updating.",
        "Released version 0.3.0!",
        "Should we use trait objects here?",
        "The CI pipeline is green.",
        "Interesting approach, let me think about it. Actually, looking at it more carefully I think there might be a subtle soundness issue with the unsafe block on line 247 — the pointer could be dangling if the Vec reallocates between the two calls.",
        "Check out the tracking issue: https://github.com/example-org/example-repo/issues/12345-very-long-descriptive-issue-title-about-refactoring-the-entire-event-system",
    ];
    pool[i % pool.len()].to_string()
}
