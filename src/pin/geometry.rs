use crate::geometry::{Point, Rect};
use crate::overlay::{edit, toolbar};

pub const MIN_SCALE: f64 = 0.1;
pub const MAX_SCALE: f64 = 4.0;
pub const SCALE_STEP: f64 = 0.1;

pub const OPACITY_MIN: u8 = 10;
pub const OPACITY_MAX: u8 = 100;
pub const OPACITY_STEP: u8 = 10;

const CLOSE_SIZE: i32 = 22;
const CLOSE_MARGIN: i32 = 4;
const HANDLE_TOLERANCE: i32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinHit {
    Close,
    Move,
    Resize(edit::Handle),
}

pub fn close_rect(width: i32, height: i32, dpi: u32) -> Rect {
    let width = width.max(1);
    let height = height.max(1);
    let margin_x = toolbar::scale(CLOSE_MARGIN, dpi).min(width / 4);
    let margin_y = toolbar::scale(CLOSE_MARGIN, dpi).min(height / 4);
    let available_w = (width - margin_x * 2).max(1);
    let available_h = (height - margin_y * 2).max(1);
    let size = toolbar::scale(CLOSE_SIZE, dpi).clamp(1, available_w.min(available_h));
    Rect::new(
        width - margin_x - size,
        margin_y,
        width - margin_x,
        margin_y + size,
    )
}

pub fn handle_tolerance(width: i32, height: i32, dpi: u32) -> i32 {
    let edge = (width.max(1).min(height.max(1)) / 4).max(1);
    toolbar::scale(HANDLE_TOLERANCE, dpi).max(1).min(edge)
}

pub fn hit_test(width: i32, height: i32, dpi: u32, point: Point) -> Option<PinHit> {
    let width = width.max(1);
    let height = height.max(1);
    if close_rect(width, height, dpi).contains(point) {
        return Some(PinHit::Close);
    }
    let local = Rect::new(0, 0, width, height);
    edit::hit_test(local, point, handle_tolerance(width, height, dpi)).map(|hit| match hit {
        edit::Hit::Move => PinHit::Move,
        edit::Hit::Resize(handle) => PinHit::Resize(handle),
    })
}

/// Moves `rect` inside `bounds` without changing its size. A rectangle larger
/// than the bounds is aligned with the top-left corner rather than inverted.
pub fn clamp_inside(rect: Rect, bounds: Rect) -> Rect {
    if rect.is_empty() {
        return rect;
    }
    if bounds.is_empty() {
        return rect;
    }
    let width = rect.width();
    let height = rect.height();
    let max_left = (bounds.right - width).max(bounds.left);
    let max_top = (bounds.bottom - height).max(bounds.top);
    let left = rect.left.clamp(bounds.left, max_left);
    let top = rect.top.clamp(bounds.top, max_top);
    Rect::new(left, top, left + width, top + height)
}

/// Clamps to the virtual desktop and, when the monitor layout leaves a
/// non-rectangular hole, makes sure the pin still intersects a monitor.
pub fn ensure_visible(rect: Rect, monitors: &[Rect], bounds: Rect) -> Rect {
    let rect = clamp_inside(rect, bounds);
    if monitors.is_empty() || monitors.iter().any(|monitor| rect.intersects(*monitor)) {
        return rect;
    }
    let Some(nearest) = monitors
        .iter()
        .min_by_key(|monitor| monitor.nearest_distance_sq(rect.center()))
    else {
        return rect;
    };
    let center = nearest.center();
    let left = center.x - rect.width() / 2;
    let top = center.y - rect.height() / 2;
    clamp_inside(
        Rect::new(left, top, left + rect.width(), top + rect.height()),
        bounds,
    )
}

pub fn scale_limits(source_w: u32, source_h: u32, bounds: Rect) -> (f64, f64) {
    let source_w = source_w.max(1) as f64;
    let source_h = source_h.max(1) as f64;
    let mut max = MAX_SCALE;
    if !bounds.is_empty() {
        max = max
            .min(bounds.width() as f64 / source_w)
            .min(bounds.height() as f64 / source_h);
    }
    if max < MIN_SCALE {
        max = MIN_SCALE;
    }
    (MIN_SCALE, max)
}

pub fn scaled_size(source_w: u32, source_h: u32, scale: f64) -> (i32, i32) {
    let scale = scale.max(0.0);
    let width = (source_w.max(1) as f64 * scale).round() as i64;
    let height = (source_h.max(1) as f64 * scale).round() as i64;
    (
        width.clamp(1, i32::MAX as i64) as i32,
        height.clamp(1, i32::MAX as i64) as i32,
    )
}

pub fn zoom_rect(
    current: Rect,
    source_w: u32,
    source_h: u32,
    new_scale: f64,
    bounds: Rect,
    monitors: &[Rect],
) -> Rect {
    let (width, height) = scaled_size(source_w, source_h, new_scale);
    let center = current.center();
    let left = center.x - width / 2;
    let top = center.y - height / 2;
    ensure_visible(
        Rect::new(left, top, left + width, top + height),
        monitors,
        bounds,
    )
}

/// Resizes with a locked source aspect ratio. The dragged handle determines
/// the new scale, the opposite edge/corner stays anchored, and the free axis
/// stays centered.
pub fn resize_rect_aspect(
    current: Rect,
    delta: Point,
    handle: edit::Handle,
    source_w: u32,
    source_h: u32,
    bounds: Rect,
    monitors: &[Rect],
) -> Rect {
    if current.is_empty() {
        return current;
    }
    let raw = edit::resize_rect(current, delta, handle, bounds);
    let moves_horizontal = matches!(
        handle,
        edit::Handle::West
            | edit::Handle::East
            | edit::Handle::NorthWest
            | edit::Handle::NorthEast
            | edit::Handle::SouthWest
            | edit::Handle::SouthEast
    );
    let moves_vertical = matches!(
        handle,
        edit::Handle::North
            | edit::Handle::South
            | edit::Handle::NorthWest
            | edit::Handle::NorthEast
            | edit::Handle::SouthWest
            | edit::Handle::SouthEast
    );
    let scale_w = raw.width() as f64 / source_w.max(1) as f64;
    let scale_h = raw.height() as f64 / source_h.max(1) as f64;
    let scale = match (moves_horizontal, moves_vertical) {
        (true, true) => {
            let dw = (raw.width() - current.width()).abs();
            let dh = (raw.height() - current.height()).abs();
            if dw >= dh {
                scale_w
            } else {
                scale_h
            }
        }
        (true, false) => scale_w,
        (false, true) => scale_h,
        (false, false) => current.width() as f64 / source_w.max(1) as f64,
    };
    let (min, max) = scale_limits(source_w, source_h, bounds);
    let scale = scale.clamp(min, max);
    let (width, height) = scaled_size(source_w, source_h, scale);
    ensure_visible(
        anchored_rect(current, handle, width, height),
        monitors,
        bounds,
    )
}

fn anchored_rect(current: Rect, handle: edit::Handle, width: i32, height: i32) -> Rect {
    let center = current.center();
    let (left, top) = match handle {
        edit::Handle::East => (current.left, center.y - height / 2),
        edit::Handle::West => (current.right - width, center.y - height / 2),
        edit::Handle::South => (center.x - width / 2, current.top),
        edit::Handle::North => (center.x - width / 2, current.bottom - height),
        edit::Handle::SouthEast => (current.left, current.top),
        edit::Handle::NorthWest => (current.right - width, current.bottom - height),
        edit::Handle::NorthEast => (current.left, current.bottom - height),
        edit::Handle::SouthWest => (current.right - width, current.top),
    };
    Rect::new(left, top, left + width, top + height)
}

pub fn opacity_alpha(percent: u8) -> u8 {
    let percent = percent.clamp(OPACITY_MIN, OPACITY_MAX) as u32;
    ((percent * 255 + 50) / 100) as u8
}

pub fn step_opacity(current: u8, steps: i32) -> u8 {
    let current = current.clamp(OPACITY_MIN, OPACITY_MAX) as i32;
    (current + steps * OPACITY_STEP as i32).clamp(OPACITY_MIN as i32, OPACITY_MAX as i32) as u8
}

pub fn step_scale(current: f64, steps: i32, min: f64, max: f64) -> f64 {
    (current + steps as f64 * SCALE_STEP).clamp(min, max)
}

pub fn wheel_steps(delta: i16) -> i32 {
    let delta = delta as i32;
    if delta == 0 {
        0
    } else if delta.abs() < 120 {
        delta.signum()
    } else {
        delta / 120
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, x + w, y + h)
    }

    #[test]
    fn close_button_sits_in_the_top_right_and_stays_inside() {
        let close = close_rect(200, 100, 96);
        assert_eq!(close.right, 196);
        assert_eq!(close.top, 4);
        assert!(close.left > 150);
        assert!(close.right <= 200);
        assert!(close.bottom <= 100);

        let tiny = close_rect(4, 3, 96);
        assert!(tiny.left >= 0 && tiny.top >= 0);
        assert!(tiny.right <= 4 && tiny.bottom <= 3);
        assert!(!tiny.is_empty());

        assert_eq!(close_rect(1, 1, 96), Rect::new(0, 0, 1, 1));
        let wide = close_rect(2, 1, 96);
        assert!(wide.left >= 0 && wide.top >= 0);
        assert!(wide.right <= 2 && wide.bottom <= 1);
    }

    #[test]
    fn close_button_scales_with_dpi() {
        let normal = close_rect(400, 300, 96);
        let scaled = close_rect(400, 300, 192);
        assert!(scaled.width() > normal.width());
        assert_eq!(scaled.right, 400 - 8);
    }

    #[test]
    fn close_button_wins_over_border_hit() {
        let close = close_rect(200, 100, 96);
        assert_eq!(hit_test(200, 100, 96, close.center()), Some(PinHit::Close));
    }

    #[test]
    fn hit_test_covers_move_and_all_handles() {
        let width = 200;
        let height = 100;
        assert_eq!(
            hit_test(width, height, 96, Point::new(100, 50)),
            Some(PinHit::Move)
        );
        assert_eq!(
            hit_test(width, height, 96, Point::new(100, 1)),
            Some(PinHit::Resize(edit::Handle::North))
        );
        assert_eq!(
            hit_test(width, height, 96, Point::new(100, 99)),
            Some(PinHit::Resize(edit::Handle::South))
        );
        assert_eq!(
            hit_test(width, height, 96, Point::new(1, 50)),
            Some(PinHit::Resize(edit::Handle::West))
        );
        assert_eq!(
            hit_test(width, height, 96, Point::new(199, 50)),
            Some(PinHit::Resize(edit::Handle::East))
        );
        assert_eq!(
            hit_test(width, height, 96, Point::new(1, 1)),
            Some(PinHit::Resize(edit::Handle::NorthWest))
        );
        assert_eq!(
            hit_test(width, height, 96, Point::new(199, 1)),
            Some(PinHit::Resize(edit::Handle::NorthEast))
        );
        assert_eq!(
            hit_test(width, height, 96, Point::new(1, 99)),
            Some(PinHit::Resize(edit::Handle::SouthWest))
        );
        assert_eq!(
            hit_test(width, height, 96, Point::new(199, 99)),
            Some(PinHit::Resize(edit::Handle::SouthEast))
        );
    }

    #[test]
    fn handle_tolerance_is_capped_for_tiny_windows() {
        assert_eq!(handle_tolerance(200, 100, 96), 6);
        assert_eq!(handle_tolerance(12, 12, 96), 3);
        assert_eq!(handle_tolerance(4, 4, 96), 1);
    }

    #[test]
    fn clamp_inside_keeps_size() {
        let bounds = Rect::new(-100, -50, 300, 250);
        let rect = Rect::new(250, 200, 350, 300);
        let clamped = clamp_inside(rect, bounds);
        assert_eq!(clamped, Rect::new(200, 150, 300, 250));
        assert_eq!(clamped.width(), rect.width());
        assert_eq!(clamped.height(), rect.height());

        let huge = Rect::new(0, 0, 500, 500);
        assert_eq!(clamp_inside(huge, bounds), Rect::new(-100, -50, 400, 450));
    }

    #[test]
    fn ensure_visible_moves_a_pin_out_of_a_monitor_gap() {
        let bounds = Rect::new(0, 0, 3000, 1200);
        let monitors = [monitor(0, 0, 1000, 1200), monitor(2000, 0, 1000, 1200)];
        let in_gap = Rect::new(1200, 500, 1400, 600);
        let result = ensure_visible(in_gap, &monitors, bounds);
        assert!(monitors.iter().any(|m| result.intersects(*m)));
        assert_eq!(result.width(), 200);
        assert_eq!(result.height(), 100);
    }

    #[test]
    fn ensure_visible_keeps_a_valid_pin() {
        let bounds = Rect::new(0, 0, 2000, 1000);
        let monitors = [monitor(0, 0, 2000, 1000)];
        let rect = Rect::new(100, 100, 300, 200);
        assert_eq!(ensure_visible(rect, &monitors, bounds), rect);
    }

    #[test]
    fn scale_limits_respect_source_and_desktop() {
        let bounds = Rect::new(0, 0, 400, 300);
        let (min, max) = scale_limits(100, 100, bounds);
        assert_eq!(min, 0.1);
        assert_eq!(max, 3.0);

        let (min, max) = scale_limits(100, 100, Rect::new(0, 0, 10, 10));
        assert_eq!(min, 0.1);
        assert_eq!(max, 0.1);

        let (min, max) = scale_limits(100, 100, Rect::new(0, 0, 2000, 2000));
        assert_eq!(min, 0.1);
        assert_eq!(max, 4.0);
    }

    #[test]
    fn scaled_size_rounds_and_keeps_ratio() {
        assert_eq!(scaled_size(200, 100, 1.0), (200, 100));
        assert_eq!(scaled_size(200, 100, 0.5), (100, 50));
        assert_eq!(scaled_size(200, 100, 1.25), (250, 125));
        assert_eq!(scaled_size(1, 1, 0.1), (1, 1));
    }

    #[test]
    fn zoom_is_center_anchored_and_clamped() {
        let bounds = Rect::new(0, 0, 1000, 1000);
        let monitors = [monitor(0, 0, 1000, 1000)];
        let current = Rect::new(300, 300, 500, 400);
        let zoomed = zoom_rect(current, 200, 100, 2.0, bounds, &monitors);
        assert_eq!(zoomed.width(), 400);
        assert_eq!(zoomed.height(), 200);
        assert_eq!(zoomed.center(), current.center());

        let edge = Rect::new(0, 0, 200, 100);
        let zoomed = zoom_rect(edge, 200, 100, 2.0, bounds, &monitors);
        assert_eq!(zoomed.left, 0);
        assert_eq!(zoomed.top, 0);
        assert_eq!(zoomed.width(), 400);
        assert_eq!(zoomed.height(), 200);
    }

    #[test]
    fn wheel_scale_steps_and_limits() {
        assert!((step_scale(1.0, 1, 0.1, 4.0) - 1.1).abs() < 1e-9);
        assert!((step_scale(1.0, -1, 0.1, 4.0) - 0.9).abs() < 1e-9);
        assert_eq!(step_scale(0.1, -1, 0.1, 4.0), 0.1);
        assert_eq!(step_scale(4.0, 1, 0.1, 4.0), 4.0);
    }

    #[test]
    fn aspect_resize_anchors_opposite_edges() {
        let bounds = Rect::new(0, 0, 2000, 2000);
        let monitors = [monitor(0, 0, 2000, 2000)];
        let current = Rect::new(100, 100, 300, 200);

        let east = resize_rect_aspect(
            current,
            Point::new(40, 0),
            edit::Handle::East,
            200,
            100,
            bounds,
            &monitors,
        );
        assert_eq!(east.width(), 240);
        assert_eq!(east.height(), 120);
        assert_eq!(east.left, 100);
        assert_eq!(east.center().y, current.center().y);

        let south = resize_rect_aspect(
            current,
            Point::new(0, 30),
            edit::Handle::South,
            200,
            100,
            bounds,
            &monitors,
        );
        assert_eq!(south.width(), 260);
        assert_eq!(south.height(), 130);
        assert_eq!(south.top, 100);
        assert_eq!(south.center().x, current.center().x);

        let nw = resize_rect_aspect(
            current,
            Point::new(-40, -20),
            edit::Handle::NorthWest,
            200,
            100,
            bounds,
            &monitors,
        );
        assert_eq!(nw.width(), 240);
        assert_eq!(nw.height(), 120);
        assert_eq!(nw.right, 300);
        assert_eq!(nw.bottom, 200);
    }

    #[test]
    fn aspect_resize_uses_the_dominant_axis_for_corners() {
        let bounds = Rect::new(0, 0, 2000, 2000);
        let monitors = [monitor(0, 0, 2000, 2000)];
        let current = Rect::new(100, 100, 300, 200);
        let result = resize_rect_aspect(
            current,
            Point::new(80, 5),
            edit::Handle::SouthEast,
            200,
            100,
            bounds,
            &monitors,
        );
        assert_eq!(result.width(), 280);
        assert_eq!(result.height(), 140);
        assert_eq!(result.left, 100);
        assert_eq!(result.top, 100);
    }

    #[test]
    fn aspect_resize_clamps_scale_and_bounds() {
        let bounds = Rect::new(0, 0, 500, 500);
        let monitors = [monitor(0, 0, 500, 500)];
        let current = Rect::new(100, 100, 300, 200);
        let huge = resize_rect_aspect(
            current,
            Point::new(5000, 5000),
            edit::Handle::SouthEast,
            200,
            100,
            bounds,
            &monitors,
        );
        assert!(huge.width() <= bounds.width());
        assert!(huge.height() <= bounds.height());
        assert_eq!(huge.width(), huge.height() * 2);

        let tiny = resize_rect_aspect(
            current,
            Point::new(-5000, -5000),
            edit::Handle::SouthEast,
            200,
            100,
            bounds,
            &monitors,
        );
        assert_eq!(tiny.width(), 20);
        assert_eq!(tiny.height(), 10);
    }

    #[test]
    fn opacity_steps_and_alpha_mapping() {
        assert_eq!(opacity_alpha(100), 255);
        assert_eq!(opacity_alpha(50), 128);
        assert_eq!(opacity_alpha(10), 26);
        assert_eq!(step_opacity(100, 1), 100);
        assert_eq!(step_opacity(100, -1), 90);
        assert_eq!(step_opacity(10, -1), 10);
        assert_eq!(step_opacity(50, 2), 70);
    }

    #[test]
    fn wheel_delta_maps_to_discrete_steps() {
        assert_eq!(wheel_steps(0), 0);
        assert_eq!(wheel_steps(120), 1);
        assert_eq!(wheel_steps(-120), -1);
        assert_eq!(wheel_steps(240), 2);
        assert_eq!(wheel_steps(60), 1);
        assert_eq!(wheel_steps(-60), -1);
    }
}
