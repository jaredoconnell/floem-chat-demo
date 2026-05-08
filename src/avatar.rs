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
pub fn user_avatar(username: &str) -> impl IntoView {
    let letter = username.chars().next().unwrap_or('?').to_string();
    let hash = username
        .bytes()
        .fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    let (r, g, b) = AVATAR_COLORS[hash % AVATAR_COLORS.len()];
    let bg = Color::from_rgb8(r, g, b);

    Label::new(letter).style(move |s| {
        s.width(32.0)
            .min_width(32.0)
            .height(32.0)
            .min_height(32.0)
            .border_radius(16.0)
            .background(bg)
            .color(Color::WHITE)
            .justify_center()
            .items_center()
            .font_size(16.0)
    })
}
