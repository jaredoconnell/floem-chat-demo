//! Card-stack layout algorithms for docked panes.
//!
//! ## Overview
//!
//! These functions compute `target_x` positions for panes so they pack from
//! the right edge of the window. When the total width of all docked panes
//! exceeds the window width, overflow panes are compressed into narrow
//! "peek strips" at the left and right edges — like a fanned deck of cards.
//!
//! ## Key algorithm: `assign_card_stack_positions`
//!
//! 1. Start from the focused pane and expand a "visible range" outward.
//! 2. Visible panes get their full width; overflow panes get `PEEK_WIDTH`.
//! 3. Right-overflow panes (lower `dock_order`) stack on the right edge.
//! 4. Left-overflow panes (higher `dock_order`) stack on the left edge.
//!
//! ## Browser pane pinning
//!
//! The browser pane is always pinned to the right edge and doesn't
//! participate in the card-stack layout. This ensures the server/channel
//! browser is always accessible.
//!
//! ## Floem concept: `set_input_regions`
//!
//! `floem::action::set_input_regions(Some(rects))` tells the windowing
//! system (via winit) which rectangular regions of the window should
//! receive mouse input. Everything outside these regions is click-through.
//! Used in "pseudo-window" mode where the window is transparent and only
//! the panes themselves are interactive.
//! `set_input_regions(None)` disables this and makes the whole window
//! receive input (normal mode).

use floem::action::set_input_regions;

use super::model::*;

/// Returns the visible center of a pane, accounting for card-stack compression.
///
/// When a pane is stacked (compressed into a peek strip), its logical center
/// is the middle of the visible strip, not the middle of its full width.
/// Used during drag reorder to determine where to insert the dragged pane.
pub fn visible_center(pane: &PaneState) -> f64 {
    match pane.stack_side {
        Some(StackSide::Right) => pane.target_x + pane.width - PEEK_WIDTH / 2.0,
        Some(StackSide::Left) => pane.target_x + PEEK_WIDTH / 2.0,
        None => pane.target_x + pane.width / 2.0,
    }
}

/// Pin the browser pane to the right edge and return the remaining
/// width available for other panes.
///
/// The browser pane is always positioned at `window_width - width - PANE_SPACING`
/// and doesn't participate in the card-stack layout. This helper is shared
/// by both `recompute_dock_targets` and `recompute_targets_during_drag` to
/// avoid duplicating the browser-pinning logic.
fn pin_browser_pane(
    panes: &mut [PaneState],
    window_width: f64,
    exclude_id: Option<usize>,
) -> (Option<usize>, f64) {
    // Find the docked browser pane (if any), excluding the dragged pane.
    let browser_idx = panes.iter().position(|p| {
        p.docked
            && matches!(p.kind, PaneKind::Browser)
            && exclude_id.map_or(true, |eid| p.id != eid)
    });
    let effective_width = if let Some(bi) = browser_idx {
        let bw = panes[bi].width;
        panes[bi].target_x = window_width - bw - PANE_SPACING;
        panes[bi].stack_side = None;
        // z_order 200 = same level as other fully visible panes.
        panes[bi].z_order = 200;
        // Remaining width is everything left of the browser pane.
        window_width - bw - PANE_SPACING
    } else {
        window_width
    };
    (browser_idx, effective_width)
}

/// Pack docked panes from the right edge. When the total width exceeds the
/// window, overflow panes compress into peek strips at the left/right edges
/// like a fanned deck of cards. ``focus_id`` determines which pane stays
/// fully visible (the most recently opened or interacted-with pane).
///
/// This is the main entry point — called after adding/removing panes,
/// after window resize, after drag end, etc.
pub fn recompute_dock_targets(
    panes: &mut [PaneState],
    window_width: f64,
    focus_id: Option<usize>,
) {
    // Step 1: Pin the browser pane to the right edge. It doesn't participate
    // in the card-stack layout — position it first, then lay out the rest
    // in the remaining space.
    let (browser_idx, effective_width) = pin_browser_pane(panes, window_width, None);

    // Step 2: Collect indices of docked non-browser panes, sorted by dock_order.
    // Lower dock_order = further right in the window.
    let mut docked: Vec<usize> = panes
        .iter()
        .enumerate()
        .filter(|(i, p)| p.docked && Some(*i) != browser_idx)
        .map(|(i, _)| i)
        .collect();
    docked.sort_by_key(|&i| panes[i].dock_order);

    let n = docked.len();
    if n == 0 {
        return;
    }

    // Reset stack_side/z_order for all docked panes before recomputing.
    for &i in &docked {
        panes[i].stack_side = None;
        panes[i].z_order = 0;
    }

    // Step 3: Check if everything fits at full width.
    let total: f64 =
        docked.iter().map(|&i| panes[i].width).sum::<f64>() + (n as f64) * PANE_SPACING;

    if total <= effective_width {
        // Everything fits — simple right-packing. Start from the right edge
        // and place each pane moving leftward.
        let mut cursor = effective_width;
        for &i in &docked {
            cursor -= panes[i].width + PANE_SPACING;
            panes[i].target_x = cursor;
        }
        return;
    }

    // Step 4: Overflow — use the card-stack algorithm to compress.
    assign_card_stack_positions(&docked, panes, effective_width, focus_id);
}

/// Core card-stack positioning. Determines which panes are fully visible and
/// which are compressed into peek strips at the edges.
///
/// ## Algorithm
///
/// 1. Find the focus pane in the sorted dock_order list.
/// 2. Expand a "visible range" outward from the focus pane: try adding
///    one pane to the right, then one to the left, repeating until no
///    more fit. "Fit" means the visible panes plus peek strips for all
///    overflow panes don't exceed `window_width`.
/// 3. Position visible panes at full width, packed from the right.
/// 4. Position right-overflow panes as peek strips adjacent to the
///    rightmost visible pane.
/// 5. Position left-overflow panes as peek strips adjacent to the
///    leftmost visible pane.
///
/// ``docked`` must be sorted by dock_order ascending. Lower dock_order = further
/// right in the window. Overflow panes with lower dock_order than the visible
/// range stack on the right edge; higher dock_order overflow stacks on the left.
pub fn assign_card_stack_positions(
    docked: &[usize],
    panes: &mut [PaneState],
    window_width: f64,
    focus_id: Option<usize>,
) {
    let n = docked.len();

    // Find the focus pane in the sorted order; default to the highest
    // dock_order (newest, leftmost) if not found.
    let focus_sorted_idx = focus_id
        .and_then(|fid| docked.iter().position(|&i| panes[i].id == fid))
        .unwrap_or(n - 1);

    // Expand the visible range outward from the focus pane, preferring
    // rightward expansion first to keep the right-aligned feel.
    let mut vis_start = focus_sorted_idx;
    let mut vis_end = focus_sorted_idx;
    let mut used_width = panes[docked[focus_sorted_idx]].width;

    loop {
        let mut expanded = false;

        // Try expanding right (lower dock_order direction — toward the
        // right edge of the window, i.e. toward index 0 in sorted order).
        if vis_start > 0 {
            let candidate_w = panes[docked[vis_start - 1]].width + PANE_SPACING;
            let left_count = vis_start - 1;
            let right_count = n - vis_end - 1;
            // Available space = window_width minus peek strips for all overflow panes.
            let available =
                window_width - left_count as f64 * PEEK_WIDTH - right_count as f64 * PEEK_WIDTH;
            if used_width + candidate_w <= available {
                vis_start -= 1;
                used_width += candidate_w;
                expanded = true;
            }
        }

        // Try expanding left (higher dock_order — toward the left edge).
        if vis_end + 1 < n {
            let candidate_w = panes[docked[vis_end + 1]].width + PANE_SPACING;
            let left_count = vis_start;
            let right_count = n - vis_end - 2;
            let available =
                window_width - left_count as f64 * PEEK_WIDTH - right_count as f64 * PEEK_WIDTH;
            if used_width + candidate_w <= available {
                vis_end += 1;
                used_width += candidate_w;
                expanded = true;
            }
        }

        if !expanded {
            break;
        }
    }

    // docked[..vis_start] = lower dock_order = rightward in window → RIGHT stack
    // docked[vis_end+1..] = higher dock_order = leftward in window → LEFT stack
    let right_count = vis_start;
    let left_count = n - vis_end - 1;

    // --- Position visible panes: pack from right within the available space ---
    // Visible panes render above stacked peek strips (stacked z tops out
    // around 100 + stack_count, so 200 is safely above).
    // Only add PANE_SPACING between consecutive visible panes.
    let mut cursor = window_width;
    for (idx, &i) in docked[vis_start..=vis_end].iter().enumerate() {
        let spacing = if idx == 0 { 0.0 } else { PANE_SPACING };
        cursor -= panes[i].width + spacing;
        panes[i].target_x = cursor;
        panes[i].stack_side = None;
        panes[i].z_order = 200;
    }

    // Shift visible panes so the stacked tabs fit adjacent on both sides.
    // Without this shift, visible panes would overlap with the peek strips.
    let vis_right_edge = window_width; // rightmost pane's right edge before shift
    let vis_left_edge = cursor; // leftmost pane's left edge before shift
    let needed_right = right_count as f64 * PEEK_WIDTH;
    let needed_left = left_count as f64 * PEEK_WIDTH;
    let total_needed = needed_left + needed_right
        + (vis_right_edge - vis_left_edge); // width of visible block
    if total_needed <= window_width {
        // The visible block + all peek strips fit. Push visible panes
        // left by `needed_right` to make room for right peek strips.
        let shift_for_right = -needed_right;
        let shifted_left_edge = vis_left_edge + shift_for_right;
        // If this pushes us past the left peek strips, shift right to compensate.
        let shift_for_left = if shifted_left_edge < needed_left {
            needed_left - shifted_left_edge
        } else {
            0.0
        };
        let total_shift = shift_for_right + shift_for_left;
        for &i in &docked[vis_start..=vis_end] {
            panes[i].target_x += total_shift;
        }
    } else {
        // Not enough room even for stacks; just ensure left stacks fit.
        if cursor < needed_left {
            let s = needed_left - cursor;
            for &i in &docked[vis_start..=vis_end] {
                panes[i].target_x += s;
            }
        }
    }

    // Compute actual visible edges after shifting.
    let vis_left = panes[docked[vis_end]].target_x;
    let vis_right = panes[docked[vis_start]].target_x + panes[docked[vis_start]].width;

    // --- Position right stack adjacent to the rightmost visible pane ---
    // Iterate in reverse so that panes nearest to vis_start (closest in
    // dock_order to the visible range) are nearest to visible visually.
    // k=0 nearest to visible (highest z), k=right_count-1 outermost.
    for (k, &sorted_idx) in docked[..vis_start].iter().rev().enumerate() {
        // Position so only PEEK_WIDTH of the pane is visible, extending
        // rightward from the right edge of the visible block.
        panes[sorted_idx].target_x =
            vis_right + k as f64 * PEEK_WIDTH - panes[sorted_idx].width + PEEK_WIDTH;
        panes[sorted_idx].stack_side = Some(StackSide::Right);
        // z_order: panes closer to visible get higher z so they render on top
        // of panes further away.
        panes[sorted_idx].z_order = (right_count - k) as i32 + 100;
    }

    // --- Position left stack adjacent to the leftmost visible pane ---
    // k=0 nearest to visible (highest z), k=left_count-1 outermost.
    for (k, &sorted_idx) in docked[vis_end + 1..].iter().enumerate() {
        panes[sorted_idx].target_x = vis_left - (k as f64 + 1.0) * PEEK_WIDTH;
        panes[sorted_idx].stack_side = Some(StackSide::Left);
        panes[sorted_idx].z_order = (left_count - k) as i32 + 100;
    }
}

/// While dragging (or resizing), figure out where the active pane should
/// slot in among the other docked panes, then re-lay-out using the
/// card-stack algorithm with the dragged pane as the focus.
///
/// ## Reorder heuristic
///
/// Uses a "leading edge" threshold: the dragged pane's right edge must
/// pass 50% of the neighbor's visible width to trigger a swap. The
/// `last_insert_pos` provides hysteresis — once a swap commits, the
/// pane must cross back past the neighbor to undo it, preventing jitter.
pub fn recompute_targets_during_drag(
    panes: &mut [PaneState],
    drag_id: usize,
    window_width: f64,
    last_insert_pos: &mut Option<usize>,
) {
    let drag_idx = panes.iter().position(|p| p.id == drag_id);
    let Some(drag_idx) = drag_idx else { return };

    if !panes[drag_idx].docked {
        // Dragged pane is floating — just re-layout the remaining docked
        // panes using the last known focus (pick newest docked as fallback).
        let fallback = panes
            .iter()
            .filter(|p| p.docked && p.id != drag_id)
            .max_by_key(|p| p.dock_order)
            .map(|p| p.id);
        recompute_dock_targets(panes, window_width, fallback);
        return;
    }

    // Pin the browser pane to the right.
    let (browser_idx, effective_width) = pin_browser_pane(panes, window_width, Some(drag_id));

    // Collect docked non-browser panes excluding the dragged one.
    let mut others: Vec<usize> = panes
        .iter()
        .enumerate()
        .filter(|(i, p)| p.docked && p.id != drag_id && Some(*i) != browser_idx)
        .map(|(i, _)| i)
        .collect();
    others.sort_by_key(|&i| panes[i].dock_order);

    if others.is_empty() {
        return;
    }

    // Determine where the dragged pane should be inserted based on its
    // current position relative to the other panes' visible centers.
    let drag_left = panes[drag_idx].x;
    let drag_right = drag_left + panes[drag_idx].width;

    let prev = match *last_insert_pos {
        Some(p) => p.min(others.len()),
        None => {
            // Initial placement: use right edge vs neighbor centers.
            others
                .iter()
                .position(|&i| {
                    drag_right > panes[i].target_x + panes[i].width / 2.0
                })
                .unwrap_or(others.len())
        }
    };

    let mut insert_pos = prev;

    // Try move RIGHT (decrease pos): drag's right edge passes 50% of right neighbor.
    // For stacked panes, use the visible strip center instead of full logical center.
    if prev > 0 {
        let i = others[prev - 1];
        if drag_right > visible_center(&panes[i]) {
            insert_pos = prev - 1;
        }
    }

    // Try move LEFT (increase pos): drag's left edge passes 50% of left neighbor.
    if insert_pos == prev && prev < others.len() {
        let i = others[prev];
        if drag_left < visible_center(&panes[i]) {
            insert_pos = prev + 1;
        }
    }

    // Commit the insert position for hysteresis (next frame uses this as baseline).
    *last_insert_pos = Some(insert_pos);

    // Build the full ordered list with the dragged pane inserted at its
    // computed position, then run the card-stack algorithm.
    let mut ordered: Vec<usize> = Vec::with_capacity(others.len() + 1);
    ordered.extend_from_slice(&others[..insert_pos]);
    ordered.push(drag_idx);
    ordered.extend_from_slice(&others[insert_pos..]);

    // Use the card-stack algorithm with the dragged pane as focus.
    assign_card_stack_positions(&ordered, panes, effective_width, Some(drag_id));
    // The dragged pane's position is mouse-controlled, not layout-controlled,
    // so snap its target_x to its current x.
    panes[drag_idx].target_x = panes[drag_idx].x;
    // Dragged pane always renders on top of everything.
    panes[drag_idx].z_order = 300;
}

/// After a drag ends, commit the dragged pane's spatial position into the
/// dock_order values so that subsequent re-layouts preserve the new ordering.
///
/// Without this, `recompute_dock_targets` would use the old dock_order values
/// and panes would snap back to their pre-drag positions.
pub fn commit_drag_order(panes: &mut [PaneState], drag_id: usize) {
    let drag_idx = panes.iter().position(|p| p.id == drag_id);
    let Some(drag_idx) = drag_idx else { return };
    if !panes[drag_idx].docked {
        return;
    }

    // Exclude the browser pane — it's pinned and doesn't participate in ordering.
    let mut others: Vec<usize> = panes
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.docked && p.id != drag_id && !matches!(p.kind, PaneKind::Browser)
        })
        .map(|(i, _)| i)
        .collect();
    others.sort_by_key(|&i| panes[i].dock_order);

    // Leading-edge logic matching recompute_targets_during_drag:
    // use visible center for stacked panes.
    let drag_right = panes[drag_idx].x + panes[drag_idx].width;
    let insert_pos = others
        .iter()
        .position(|&i| drag_right > visible_center(&panes[i]))
        .unwrap_or(others.len());

    let mut ordered: Vec<usize> = Vec::with_capacity(others.len() + 1);
    ordered.extend_from_slice(&others[..insert_pos]);
    ordered.push(drag_idx);
    ordered.extend_from_slice(&others[insert_pos..]);

    // Reassign contiguous dock_order values to match the new spatial order.
    // This "bakes in" the drag result so future re-layouts preserve it.
    for (rank, &idx) in ordered.iter().enumerate() {
        panes[idx].dock_order = rank;
    }
}

/// Compute the set of rectangles (in window-local logical coords) that
/// should receive mouse input, and push them to Floem via `set_input_regions`.
///
/// Called whenever pane geometry or window size changes (during animation
/// or after resize). Only effective in pseudo-window mode — in normal mode,
/// `set_input_regions(None)` is used to accept all input.
///
/// ## Floem concept: `set_input_regions`
///
/// This is a platform-specific Floem action that configures which parts of
/// a transparent window should receive mouse events. It maps to
/// `winit::Window::set_cursor_hit_test` or platform-specific equivalents
/// on macOS/Windows. Regions are specified as `kurbo::Rect`s in logical pixels.
pub fn update_input_regions(panes: &[PaneState], window_width: f64, window_height: f64) {
    use floem::kurbo::Rect;
    let mut regions = Vec::with_capacity(panes.len() + 1);
    // Toolbar strip across the top — always interactive.
    regions.push(Rect::new(0.0, 0.0, window_width, TOOLBAR_HEIGHT));
    // Each pane gets its own interactive region.
    for ps in panes {
        let dh = if ps.collapsed {
            PANE_HEADER_HEIGHT
        } else {
            ps.height
        };
        let top = if ps.docked { window_height - dh } else { ps.y };
        regions.push(Rect::new(ps.x, top, ps.x + ps.width, top + dh));
    }
    set_input_regions(Some(regions));
}
