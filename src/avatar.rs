//! Deterministic colored-circle avatars.
//!
//! ## Floem concepts demonstrated
//!
//! - **`Label::new(text)`** — the simplest Floem view: renders a string.
//! - **`.style(move |s| ...)`** — a *reactive style closure*. Floem calls
//!   this closure to compute styles, and re-calls it whenever any signal
//!   read inside it changes. Here we use a `move` closure to capture
//!   the precomputed `bg` color. Since no signals are read, this closure
//!   runs exactly once.
//! - **`impl IntoView`** — Floem's trait for "anything that can become a
//!   view". Functions return `impl IntoView` so callers don't need to know
//!   the concrete type. This is similar to SwiftUI's `some View`.

use floem::prelude::*;

/// Palette of Discord-esque avatar background colors.
const AVATAR_COLORS: &[(u8, u8, u8)] = &[
    (114, 137, 218), // blurple
    (87, 242, 135),  // green
    (254, 231, 92),  // yellow
    (237, 66, 69),   // red
    (235, 69, 158),  // fuchsia
    (88, 101, 242),  // indigo
    (249, 168, 37),  // amber
    (69, 221, 210),  // teal
];

/// Creates a 32x32 avatar view: a colored circle with the user's initial
/// centered in white. The color is deterministically chosen from a palette
/// based on a simple hash of the username.
///
/// This uses `Label` as a circle by setting equal width/height and
/// `border_radius(half)`. Floem's layout engine (Taffy) respects these
/// constraints, so the label becomes a perfect circle.
pub fn user_avatar(username: &str) -> impl IntoView {
    let letter = username.chars().next().unwrap_or('?').to_string();
    // Simple deterministic hash so the same username always gets the same color.
    let hash = username
        .bytes()
        .fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    let (r, g, b) = AVATAR_COLORS[hash % AVATAR_COLORS.len()];
    let bg = Color::from_rgb8(r, g, b);

    // `Label::new` creates a static text view.
    // The style closure captures `bg` by value (Color is Copy).
    // `justify_center` + `items_center` centers the letter inside the circle
    // (these map to CSS `justify-content` and `align-items`).
    Label::new(letter).style(move |s| {
        s.width(32.0)
            .min_width(32.0)
            .height(32.0)
            .min_height(32.0)
            .border_radius(16.0) // half of width/height = circle
            .background(bg)
            .color(Color::WHITE)
            .justify_center()
            .items_center()
            .font_size(16.0)
    })
}
