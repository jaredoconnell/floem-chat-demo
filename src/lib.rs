//! Floem Demo — a Discord-like chat UI demonstrating Floem's reactive toolkit.
//!
//! This library crate is shared by two binary entry points:
//!
//! - **`bin/unified.rs`**: Classic three-column layout (server bar, channel
//!   sidebar, chat area) in a single decorated window.
//! - **`bin/paned.rs`**: Advanced floating-pane window manager with drag,
//!   resize, card-stack overflow, and transparent click-through.
//!
//! ## Module overview
//!
//! - [`data`]: Domain models (`Server`, `Channel`, `Message`) and reactive
//!   `AppState` with sample data.
//! - [`theme`]: Centralized color palette.
//! - [`avatar`]: Deterministic colored-circle avatars.
//! - [`components`]: Reusable Floem widgets (icon circles, channel rows,
//!   headers, message rows, text input).
//! - [`server_list`], [`channel_sidebar`], [`chat_area`]: Panel-level
//!   compositions used by the unified binary.
//! - [`pane`]: The paned window manager system (model, layout algorithms,
//!   animation, and pane UI components).

pub mod avatar;
pub mod channel_sidebar;
pub mod chat_area;
pub mod components;
pub mod data;
pub mod pane;
pub mod server_list;
#[allow(dead_code)]
pub mod theme;
