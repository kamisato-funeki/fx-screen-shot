use crate::geometry::{Point, Rect};

/// Smallest selection the editor will produce.
pub const MIN_SIZE: i32 = 2;

/// Grab distance around a selection edge, in 96-DPI pixels. Callers scale this
/// value with the effective DPI before hit testing.
pub const HANDLE_TOLERANCE: i32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl Handle {
    fn moves_left(self) -> bool {
        matches!(self, Self::West | Self::NorthWest | Self::SouthWest)
    }

    fn moves_right(self) -> bool {
        matches!(self, Self::East | Self::NorthEast | Self::SouthEast)
    }

    fn moves_top(self) -> bool {
        matches!(self, Self::North | Self::NorthEast | Self::NorthWest)
    }

    fn moves_bottom(self) -> bool {
        matches!(self, Self::South | Self::SouthEast | Self::SouthWest)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Move,
    Resize(Handle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragMode {
    Move,
    Resize(Handle),
}

#[derive(Clone, Copy, Debug)]
pub struct SelectionDrag {
    pub mode: DragMode,
    pub start: Point,
    pub original: Rect,
}

/// Classifies a pointer position against the locked selection.
///
/// The selection is expanded by `tolerance` pixels so edges and corners remain
/// easy to grab from slightly outside the drawn border.
pub fn hit_test(selection: Rect, point: Point, tolerance: i32) -> Option<Hit> {
    if selection.is_empty() {
        return None;
    }
    let tolerance = tolerance.max(1);
    let expanded = Rect::new(
        selection.left - tolerance,
        selection.top - tolerance,
        selection.right + tolerance,
        selection.bottom + tolerance,
    );
    if !expanded.contains(point) {
        return None;
    }

    let dx_left = (point.x - selection.left).abs();
    let dx_right = (point.x - selection.right).abs();
    let dy_top = (point.y - selection.top).abs();
    let dy_bottom = (point.y - selection.bottom).abs();

    let near_left = dx_left <= tolerance;
    let near_right = dx_right <= tolerance;
    let near_top = dy_top <= tolerance;
    let near_bottom = dy_bottom <= tolerance;

    // Some(true) means the west edge, Some(false) the east edge. For tiny
    // selections both sides can be in range; pick the closer one.
    let west = match (near_left, near_right) {
        (true, true) => Some(dx_left <= dx_right),
        (true, false) => Some(true),
        (false, true) => Some(false),
        (false, false) => None,
    };
    // Some(true) means the north edge, Some(false) the south edge.
    let north = match (near_top, near_bottom) {
        (true, true) => Some(dy_top <= dy_bottom),
        (true, false) => Some(true),
        (false, true) => Some(false),
        (false, false) => None,
    };

    let handle = match (west, north) {
        (Some(true), Some(true)) => Some(Handle::NorthWest),
        (Some(false), Some(true)) => Some(Handle::NorthEast),
        (Some(true), Some(false)) => Some(Handle::SouthWest),
        (Some(false), Some(false)) => Some(Handle::SouthEast),
        (Some(true), None) => Some(Handle::West),
        (Some(false), None) => Some(Handle::East),
        (None, Some(true)) => Some(Handle::North),
        (None, Some(false)) => Some(Handle::South),
        (None, None) => None,
    };

    if let Some(handle) = handle {
        Some(Hit::Resize(handle))
    } else if selection.contains(point) {
        Some(Hit::Move)
    } else {
        None
    }
}

/// Translates a selection without changing its size, clamped to `bounds`.
pub fn move_rect(original: Rect, delta: Point, bounds: Rect) -> Rect {
    if original.is_empty() || bounds.is_empty() {
        return original;
    }
    let dx = clamp_offset(
        original.left,
        original.right,
        delta.x,
        bounds.left,
        bounds.right,
    );
    let dy = clamp_offset(
        original.top,
        original.bottom,
        delta.y,
        bounds.top,
        bounds.bottom,
    );
    Rect::new(
        original.left + dx,
        original.top + dy,
        original.right + dx,
        original.bottom + dy,
    )
}

/// Resizes one or two sides by `delta`, keeping the opposite sides fixed.
/// The result never becomes smaller than `MIN_SIZE` and stays inside `bounds`.
pub fn resize_rect(original: Rect, delta: Point, handle: Handle, bounds: Rect) -> Rect {
    if original.is_empty() || bounds.is_empty() {
        return original;
    }

    let mut result = original;
    if handle.moves_left() {
        let max_left = (original.right - MIN_SIZE).max(bounds.left);
        result.left = (original.left + delta.x).clamp(bounds.left, max_left);
    }
    if handle.moves_right() {
        let min_right = (original.left + MIN_SIZE).min(bounds.right);
        result.right = (original.right + delta.x).clamp(min_right, bounds.right);
    }
    if handle.moves_top() {
        let max_top = (original.bottom - MIN_SIZE).max(bounds.top);
        result.top = (original.top + delta.y).clamp(bounds.top, max_top);
    }
    if handle.moves_bottom() {
        let min_bottom = (original.top + MIN_SIZE).min(bounds.bottom);
        result.bottom = (original.bottom + delta.y).clamp(min_bottom, bounds.bottom);
    }
    result
}

fn clamp_offset(start: i32, end: i32, delta: i32, min: i32, max: i32) -> i32 {
    let min_delta = min - start;
    let max_delta = max - end;
    if min_delta > max_delta {
        // The selection is larger than the bounds; keep it where it is.
        0
    } else {
        delta.clamp(min_delta, max_delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection() -> Rect {
        Rect::new(100, 100, 300, 250)
    }

    #[test]
    fn center_is_move() {
        assert_eq!(
            hit_test(selection(), Point::new(200, 175), 5),
            Some(Hit::Move)
        );
    }

    #[test]
    fn edges_and_corners_are_resize_handles() {
        let rect = selection();
        assert_eq!(
            hit_test(rect, Point::new(100, 175), 5),
            Some(Hit::Resize(Handle::West))
        );
        assert_eq!(
            hit_test(rect, Point::new(300, 175), 5),
            Some(Hit::Resize(Handle::East))
        );
        assert_eq!(
            hit_test(rect, Point::new(200, 100), 5),
            Some(Hit::Resize(Handle::North))
        );
        assert_eq!(
            hit_test(rect, Point::new(200, 250), 5),
            Some(Hit::Resize(Handle::South))
        );
        assert_eq!(
            hit_test(rect, Point::new(100, 100), 5),
            Some(Hit::Resize(Handle::NorthWest))
        );
        assert_eq!(
            hit_test(rect, Point::new(300, 100), 5),
            Some(Hit::Resize(Handle::NorthEast))
        );
        assert_eq!(
            hit_test(rect, Point::new(100, 250), 5),
            Some(Hit::Resize(Handle::SouthWest))
        );
        assert_eq!(
            hit_test(rect, Point::new(300, 250), 5),
            Some(Hit::Resize(Handle::SouthEast))
        );
    }

    #[test]
    fn tolerance_extends_outside_the_border() {
        let rect = selection();
        assert_eq!(
            hit_test(rect, Point::new(96, 175), 5),
            Some(Hit::Resize(Handle::West))
        );
        assert_eq!(hit_test(rect, Point::new(94, 175), 5), None);
        assert_eq!(hit_test(rect, Point::new(500, 500), 5), None);
    }

    #[test]
    fn tiny_selection_picks_the_nearest_side() {
        let rect = Rect::new(100, 100, 106, 106);
        assert_eq!(
            hit_test(rect, Point::new(103, 103), 5),
            Some(Hit::Resize(Handle::NorthWest))
        );
        assert_eq!(
            hit_test(rect, Point::new(105, 105), 5),
            Some(Hit::Resize(Handle::SouthEast))
        );
    }

    #[test]
    fn move_preserves_size_and_clamps() {
        let bounds = Rect::new(0, 0, 500, 400);
        let moved = move_rect(selection(), Point::new(50, -20), bounds);
        assert_eq!(moved, Rect::new(150, 80, 350, 230));
        assert_eq!(moved.width(), selection().width());
        assert_eq!(moved.height(), selection().height());

        let clamped = move_rect(selection(), Point::new(-500, 500), bounds);
        assert_eq!(clamped, Rect::new(0, 250, 200, 400));
    }

    #[test]
    fn resize_moves_only_the_dragged_edges() {
        let bounds = Rect::new(0, 0, 800, 600);
        let rect = selection();
        assert_eq!(
            resize_rect(rect, Point::new(20, 0), Handle::East, bounds),
            Rect::new(100, 100, 320, 250)
        );
        assert_eq!(
            resize_rect(rect, Point::new(0, 30), Handle::North, bounds),
            Rect::new(100, 130, 300, 250)
        );
        assert_eq!(
            resize_rect(rect, Point::new(-20, -20), Handle::NorthWest, bounds),
            Rect::new(80, 80, 300, 250)
        );
        assert_eq!(
            resize_rect(rect, Point::new(20, 30), Handle::SouthEast, bounds),
            Rect::new(100, 100, 320, 280)
        );
    }

    #[test]
    fn resize_respects_minimum_size_and_bounds() {
        let bounds = Rect::new(0, 0, 800, 600);
        let rect = selection();
        let collapsed = resize_rect(rect, Point::new(-500, 0), Handle::East, bounds);
        assert_eq!(collapsed.width(), MIN_SIZE);

        let clamped = resize_rect(rect, Point::new(-500, -500), Handle::NorthWest, bounds);
        assert_eq!(clamped.left, bounds.left);
        assert_eq!(clamped.top, bounds.top);
        assert_eq!(clamped.right, rect.right);
        assert_eq!(clamped.bottom, rect.bottom);
    }

    #[test]
    fn resize_does_not_flip_across_the_opposite_edge() {
        let bounds = Rect::new(0, 0, 800, 600);
        let rect = selection();
        let result = resize_rect(rect, Point::new(-500, 0), Handle::West, bounds);
        assert_eq!(result.left, bounds.left);
        assert!(result.left < result.right);
    }
}
