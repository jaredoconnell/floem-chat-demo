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

/// Generates a minimal 32x32 SVG string for a user avatar.
///
/// The background color is deterministically picked from a small palette
/// using a simple hash of the username. The first character of the name
/// is rendered centered in white.
pub fn user_avatar_svg(username: &str) -> String {
    let hash = username.bytes().fold(0usize, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as usize)
    });
    let (r, g, b) = AVATAR_COLORS[hash % AVATAR_COLORS.len()];
    let letter = username.chars().next().unwrap_or('?');

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">
  <circle cx="16" cy="16" r="16" fill="rgb({r},{g},{b})"/>
  <text x="16" y="22" text-anchor="middle" fill="white" font-size="16" font-family="sans-serif">{letter}</text>
</svg>"#
    )
}
