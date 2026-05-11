//! Animation system for smooth pane sliding during drag and reorder.
//!
//! ## How Floem animation works
//!
//! Floem doesn't have built-in "animate from A to B" primitives for
//! arbitrary properties. Instead, we implement a manual animation loop:
//!
//! 1. Each pane has `x` (current) and `target_x` (desired) positions.
//! 2. `tick_animation` moves `x` toward `target_x` using ease-out math.
//! 3. `schedule_frame` uses `exec_after_animation_frame` to request a
//!    callback on the next display refresh (~16ms at 60fps).
//! 4. The callback runs `tick_animation`, bumps `anim_tick` so style
//!    closures re-evaluate, and schedules another frame if needed.
//!
//! ## `exec_after_animation_frame`
//!
//! This is Floem's equivalent of `requestAnimationFrame` in web browsers.
//! It schedules a closure to run after the current frame has been rendered
//! and the next frame is ready. The callback receives a `Duration` argument
//! (time since last frame) which we ignore since our animation is
//! frame-rate-independent via `ANIM_FACTOR`.
//!
//! ## Why `anim_tick` instead of reading `panes` directly?
//!
//! Style closures that read `panes.get()` would clone the entire
//! `Vec<PaneState>` every frame. Instead, they read `anim_tick.get()`
//! (a cheap `u64` copy) to subscribe, then use `panes.with_untracked()`
//! to borrow specific fields. This keeps per-frame overhead minimal.

use floem::action::exec_after_animation_frame;
use floem::prelude::*;

use super::PaneCtx;
use super::layout::update_input_regions;
use super::model::*;

/// Advance each non-dragged pane's ``x`` toward its ``target_x``.
///
/// ## Animation math (ease-out)
///
/// Each frame: `step = remaining_distance * ANIM_FACTOR`
/// This creates exponential decay — big jumps when far away, small
/// nudges when close. `MIN_ANIM_SPEED` prevents the animation from
/// crawling when `step` gets tiny. `ANIM_SNAP` snaps to the target
/// when close enough, preventing sub-pixel jitter.
///
/// Returns true if any pane still needs more animation.
pub fn tick_animation(panes: &mut [PaneState], drag_id: Option<usize>) -> bool {
    let mut needs_more = false;
    for pane in panes.iter_mut() {
        // Skip the dragged pane — its position is controlled by the mouse.
        if Some(pane.id) == drag_id {
            continue;
        }
        let diff = pane.target_x - pane.x;
        if diff.abs() < ANIM_SNAP {
            // Close enough — snap exactly to avoid sub-pixel jitter.
            pane.x = pane.target_x;
        } else {
            let step = diff * ANIM_FACTOR;
            // Enforce minimum speed so animation doesn't crawl
            let step = if step.abs() < MIN_ANIM_SPEED {
                MIN_ANIM_SPEED.copysign(diff)
            } else {
                step
            };
            // Clamp so the step never overshoots the target
            let step = if step.abs() > diff.abs() { diff } else { step };
            pane.x += step;
            needs_more = true;
        }
    }
    needs_more
}

/// Kick off the animation loop if one isn't already running.
///
/// The `animating` flag on `PaneCtx` acts as a mutex — if an animation
/// loop is already running, this function is a no-op. This prevents
/// multiple concurrent loops from fighting over pane positions.
pub fn start_animation(ctx: PaneCtx) {
    if ctx.animating.get_untracked() {
        return;
    }
    ctx.animating.set(true);
    schedule_frame(ctx);
}

/// Schedule a single animation frame. When the frame fires:
/// 1. Run `tick_animation` to advance all pane positions.
/// 2. Bump `anim_tick` so style closures re-evaluate.
/// 3. Update input regions (pseudo-window mode only).
/// 4. If any pane still needs animation, schedule another frame.
///    Otherwise, clear the `animating` flag.
fn schedule_frame(ctx: PaneCtx) {
    exec_after_animation_frame(move |_| {
        let drag_id = ctx.dragging.get_untracked().map(|d| d.pane_id);
        // `try_update` returns the closure's return value wrapped in Option.
        // It can fail if the signal has been disposed (shouldn't happen here).
        let needs_more = ctx
            .panes
            .try_update(|p| tick_animation(p, drag_id))
            .unwrap_or(false);
        // Bump the tick counter so style closures re-evaluate.
        // Style closures read `anim_tick.get()` to subscribe, then use
        // `panes.with_untracked()` for the actual data — avoiding a full
        // Vec clone on every frame.
        ctx.anim_tick.set(ctx.anim_tick.get_untracked() + 1);
        // Push updated input regions only in pseudo-window mode so click-through
        // areas track the moving panes.
        if ctx.pseudo_window.get_untracked() {
            let (ww, wh) = ctx.window_size.get_untracked();
            ctx.panes.with_untracked(|p| update_input_regions(p, ww, wh));
        }
        if needs_more {
            schedule_frame(ctx);
        } else {
            ctx.animating.set(false);
        }
    });
}
