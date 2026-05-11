//! Centralized color palette for the Discord-like dark theme.
//!
//! ## How Floem colors work
//!
//! `Color` is Floem's color type (re-exported from the `peniko` crate).
//! `Color::from_rgb8(r, g, b)` creates a color from 0-255 RGB values.
//! Because it's a `const fn`, colors can be defined as compile-time constants
//! and used anywhere in style closures without allocation.
//!
//! ## Why a separate theme module?
//!
//! Keeping all colors in one place makes it easy to adjust the look of the
//! entire app. Every view function references these constants by name
//! (e.g. `theme::CHAT_BG`) rather than hard-coding hex values, so swapping
//! to a light theme or adjusting contrast only requires edits here.

use floem::prelude::*;

// ---------------------------------------------------------------------------
// Shared colors — used by both the unified and paned demo binaries
// ---------------------------------------------------------------------------

/// Very dark near-black for the server icon strip on the far left.
pub const SERVER_BAR_BG: Color = Color::from_rgb8(6, 6, 8);
/// Slightly lighter dark for the channel sidebar (second column).
pub const CHANNEL_SIDEBAR_BG: Color = Color::from_rgb8(16, 16, 22);
/// Main chat area background — dark but distinguishable from the sidebar.
pub const CHAT_BG: Color = Color::from_rgb8(10, 10, 14);
/// Background for text input fields.
pub const INPUT_BG: Color = Color::from_rgb8(24, 24, 32);
/// Primary text color — off-white for readability on dark backgrounds.
pub const TEXT_PRIMARY: Color = Color::from_rgb8(220, 224, 232);
/// Muted text for timestamps, inactive channels, secondary UI elements.
pub const TEXT_MUTED: Color = Color::from_rgb8(88, 94, 110);
/// Discord's signature purple-blue accent color.
pub const BLURPLE: Color = Color::from_rgb8(120, 142, 225);
/// Subtle background shown when hovering over interactive elements.
pub const HOVER_BG: Color = Color::from_rgb8(20, 20, 28);
/// Background for currently active/selected items (e.g. active channel).
pub const ACTIVE_BG: Color = Color::from_rgb8(26, 26, 36);
/// Thin border below header bars to separate them from content.
pub const HEADER_BORDER: Color = Color::from_rgb8(4, 4, 6);
/// Generic divider line color.
pub const DIVIDER: Color = Color::from_rgb8(30, 30, 40);

// ---------------------------------------------------------------------------
// Paned mode — additional colors for the floating pane window manager
// ---------------------------------------------------------------------------

/// Border around individual pane cards.
pub const PANE_BORDER: Color = Color::from_rgb8(3, 3, 5);
/// Default header background for unfocused panes.
pub const PANE_HEADER_BG: Color = Color::from_rgb8(19, 19, 26);
/// Noticeably lighter header for the focused pane, so users can tell
/// which pane has keyboard focus at a glance.
pub const PANE_HEADER_FOCUSED_BG: Color = Color::from_rgb8(36, 36, 52);
/// Background for the toolbar strip at the top of the paned window.
pub const TOOLBAR_BG: Color = Color::from_rgb8(8, 8, 10);
