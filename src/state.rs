//! Interactive state for a [`crate::NodeGraph`]: viewport pan offset plus
//! selection / hover highlight, consumed by the graph's [`StatefulWidget`]
//! impl.
//!
//! `FlowState` is the single state handle for the stateful render path. A
//! freshly-`default`ed `FlowState` (offset `(0,0)`, no selection, no hover)
//! renders **byte-for-byte identically** to the legacy stateless render — it
//! simply delegates to [`NodeGraph::render_to`]. Only when the state is
//! non-default (non-zero offset, or a selection/hover set) does the render
//! diverge: a scrolled view is blitted from the off-screen canvas, and the
//! selected / hovered node's border is recolored.
//!
//! [`NodeGraph::render_to`]: crate::NodeGraph::render_to

use crate::id::NodeId;

/// Interactive state for [`crate::NodeGraph`]'s [`StatefulWidget`] impl.
///
/// Holds:
///   - `view_offset` — the top-left corner of the visible window inside the
///     graph's off-screen canvas (pan). `(0,0)` shows the canvas origin.
///   - `selection` — the currently selected node, if any (drawn with the
///     selection highlight style).
///   - `hover` — the node currently under the cursor, if any (drawn with the
///     hover highlight style).
///
/// The default value (`offset (0,0)`, `None`/`None`) is the **zero-change
/// safe-net**: rendering a graph with it is identical to rendering it
/// statelessly. See [`crate::NodeGraph`]'s `StatefulWidget` impl.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowState {
	/// Top-left corner of the visible window inside the graph's canvas.
	/// `(0,0)` shows the canvas origin; increasing x pans right, y pans down.
	pub view_offset: (u16, u16),
	/// The selected node id, if any. Drawn with the selection highlight.
	pub selection: Option<NodeId>,
	/// The hovered node id, if any. Drawn with the hover highlight.
	pub hover: Option<NodeId>,
}

impl FlowState {
	/// Create a fresh default state (offset `(0,0)`, no selection/hover).
	pub fn new() -> Self {
		Self::default()
	}

	/// Builder: set the viewport offset (top-left of the visible canvas window).
	#[must_use]
	pub fn view_offset(mut self, x: u16, y: u16) -> Self {
		self.view_offset = (x, y);
		self
	}

	/// Builder: set the selection (pass `None` to clear).
	#[must_use]
	pub fn select(mut self, id: Option<NodeId>) -> Self {
		self.selection = id;
		self
	}

	/// Builder: set the hover target (pass `None` to clear).
	#[must_use]
	pub fn hover(mut self, id: Option<NodeId>) -> Self {
		self.hover = id;
		self
	}

	/// Clamp-adjust the viewport offset by `(dx, dy)`, keeping it within the
	/// scrollable range of a `canvas_size` canvas viewed through a `view_size`
	/// window.
	///
	/// `max_offset = canvas_size - view_size` per axis (saturating to 0 when the
	/// view is larger than the canvas). The resulting offset is clamped to
	/// `[0, max]` after the signed delta is applied, so panning past either edge
	/// is a no-op rather than a wrap or panic.
	///
	/// This mirrors the `max_scroll` clamp used by the old `viewport.rs` example.
	pub fn pan(
		&mut self,
		dx: i32,
		dy: i32,
		canvas_size: (u16, u16),
		view_size: (u16, u16),
	) {
		let max_x = canvas_size.0.saturating_sub(view_size.0);
		let max_y = canvas_size.1.saturating_sub(view_size.1);
		let nx = apply_delta(self.view_offset.0, dx, max_x);
		let ny = apply_delta(self.view_offset.1, dy, max_y);
		self.view_offset = (nx, ny);
	}

	/// Center the viewport on the canvas-space point `target`, clamped to the
	/// valid scroll range.
	///
	/// Computes `offset = target - view_size/2` per axis (saturating), then
	/// clamps to `[0, max]` where `max = canvas_size.saturating_sub(view_size)`
	/// — the same clamp logic as [`Self::pan`]. So a `target` at the canvas
	/// origin or beyond the far edge pins to the corresponding scroll limit
	/// rather than underflowing or overshooting.
	pub fn center_on(
		&mut self,
		target: (u16, u16),
		canvas_size: (u16, u16),
		view_size: (u16, u16),
	) {
		let max_x = canvas_size.0.saturating_sub(view_size.0);
		let max_y = canvas_size.1.saturating_sub(view_size.1);
		let ox = clamp_offset(target.0.saturating_sub(view_size.0 / 2), max_x);
		let oy = clamp_offset(target.1.saturating_sub(view_size.1 / 2), max_y);
		self.view_offset = (ox, oy);
	}

	/// Pan minimally so `rect` (a region in canvas space, e.g. a node's full
	/// frame rect) is fully inside the view. Returns `true` if the offset
	/// changed.
	///
	/// If `rect` is already fully inside the current view window on both axes,
	/// this is a no-op and returns `false`. Otherwise it pans the smallest
	/// amount needed: when `rect` is off the left/top edge the view's origin is
	/// aligned with the rect's; when off the right/bottom edge the view's far
	/// edge is aligned with the rect's (`view_offset = rect.right() -
	/// view_size`). The resulting offset is clamped to `[0, max]`. When `rect`
	/// is larger than `view_size` on an axis, the view's origin is aligned with
	/// the rect's origin (top-left align) on that axis.
	pub fn ensure_visible(
		&mut self,
		rect: ratatui::layout::Rect,
		canvas_size: (u16, u16),
		view_size: (u16, u16),
	) -> bool {
		let max_x = canvas_size.0.saturating_sub(view_size.0);
		let max_y = canvas_size.1.saturating_sub(view_size.1);
		let (ox, oy) = self.view_offset;
		let nx = ensure_axis(ox, rect.x, rect.width, view_size.0, max_x);
		let ny = ensure_axis(oy, rect.y, rect.height, view_size.1, max_y);
		if (nx, ny) == (ox, oy) {
			return false;
		}
		self.view_offset = (nx, ny);
		true
	}
}

/// Clamp a `u16` offset to `[0, max]` (the valid scroll range).
fn clamp_offset(val: u16, max: u16) -> u16 {
	val.min(max)
}

/// Compute the new offset for one axis of [`FlowState::ensure_visible`].
///
/// `cur` is the current offset, `origin`/`extent` describe the rect on this
/// axis, `view` is the view's size and `max` its scroll limit. Returns the
/// minimally-panned offset (or `cur` if already visible).
fn ensure_axis(cur: u16, origin: u16, extent: u16, view: u16, max: u16) -> u16 {
	// Rect larger than (or equal to) the view: top-left align.
	if extent >= view {
		return clamp_offset(origin, max);
	}
	let far = origin.saturating_add(extent);
	let view_end = cur.saturating_add(view);
	// Already fully visible on this axis.
	if origin >= cur && far <= view_end {
		return cur;
	}
	// Off the left/top: align origin. Off the right/bottom: align far edge
	// with the view's far edge.
	let target = if origin < cur {
		origin
	} else {
		far.saturating_sub(view)
	};
	clamp_offset(target, max)
}

/// Apply a signed delta to a `u16` offset, clamping to `[0, max]`.
fn apply_delta(cur: u16, delta: i32, max: u16) -> u16 {
	let v = (cur as i32).saturating_add(delta).max(0);
	(v as u16).min(max)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_is_zeros_and_none() {
		let s = FlowState::default();
		assert_eq!(s.view_offset, (0, 0));
		assert_eq!(s.selection, None);
		assert_eq!(s.hover, None);
	}

	#[test]
	fn builders_set_fields() {
		let s = FlowState::new()
			.view_offset(3, 4)
			.select(Some(NodeId(1)))
			.hover(Some(NodeId(2)));
		assert_eq!(s.view_offset, (3, 4));
		assert_eq!(s.selection, Some(NodeId(1)));
		assert_eq!(s.hover, Some(NodeId(2)));
	}

	#[test]
	fn pan_increments_and_clamps() {
		let mut s = FlowState::new();
		// canvas 100x40, view 80x30 -> max (20,10)
		s.pan(5, 3, (100, 40), (80, 30));
		assert_eq!(s.view_offset, (5, 3));
		// pan past max clamps
		s.pan(100, 100, (100, 40), (80, 30));
		assert_eq!(s.view_offset, (20, 10));
		// pan negative clamps to 0
		s.pan(-100, -100, (100, 40), (80, 30));
		assert_eq!(s.view_offset, (0, 0));
	}

	#[test]
	fn pan_when_view_larger_than_canvas_clamps_to_zero() {
		let mut s = FlowState::new().view_offset(0, 0);
		// canvas smaller than view: max=0, any pan stays 0
		s.pan(10, 10, (20, 20), (80, 80));
		assert_eq!(s.view_offset, (0, 0));
	}

	#[test]
	fn center_on_centers_then_clamps() {
		let mut s = FlowState::new();
		// canvas 100x40, view 40x20 -> max (60,20)
		// target (50,20) -> offset (50-20, 20-10) = (30,10)
		s.center_on((50, 20), (100, 40), (40, 20));
		assert_eq!(s.view_offset, (30, 10));
		// target at origin -> offset 0 (saturating sub)
		s.center_on((0, 0), (100, 40), (40, 20));
		assert_eq!(s.view_offset, (0, 0));
		// target beyond far edge -> clamp to max (60,20)
		s.center_on((100, 40), (100, 40), (40, 20));
		assert_eq!(s.view_offset, (60, 20));
	}

	#[test]
	fn center_on_view_larger_than_canvas() {
		let mut s = FlowState::new();
		// canvas 20x20, view 80x80 -> max=0
		s.center_on((10, 10), (20, 20), (80, 80));
		assert_eq!(s.view_offset, (0, 0));
		// any target -> (0,0)
		s.center_on((1000, 1000), (20, 20), (80, 80));
		assert_eq!(s.view_offset, (0, 0));
	}

	#[test]
	fn ensure_visible_noop_when_visible() {
		// view at (10,5), view_size (40,20) -> visible x [10..50], y [5..25]
		let mut s = FlowState::new().view_offset(10, 5);
		let rect = ratatui::layout::Rect::new(20, 10, 5, 5);
		let changed = s.ensure_visible(rect, (100, 40), (40, 20));
		assert!(!changed);
		assert_eq!(s.view_offset, (10, 5));
	}

	#[test]
	fn ensure_visible_pans_minimally_left_and_right() {
		// view at (30,5), view_size (40,20) -> visible x [30..70]
		// rect at x=10 (off left edge) -> align left: offset 10
		let mut s = FlowState::new().view_offset(30, 5);
		let rect = ratatui::layout::Rect::new(10, 10, 5, 5);
		assert!(s.ensure_visible(rect, (100, 40), (40, 20)));
		assert_eq!(s.view_offset, (10, 5));

		// view at (10,5), visible x [10..50]; rect at x=48..53 (off right)
		// -> align right: offset = 53-40 = 13
		let mut s = FlowState::new().view_offset(10, 5);
		let rect = ratatui::layout::Rect::new(48, 10, 5, 5);
		assert!(s.ensure_visible(rect, (100, 40), (40, 20)));
		assert_eq!(s.view_offset, (13, 5));
	}

	#[test]
	fn ensure_visible_oversized_rect_aligns_top_left() {
		// rect width 60 > view width 40 -> top-left align on x
		// view at (30,5), rect at x=50 -> offset becomes 50 (clamped, max=60)
		let mut s = FlowState::new().view_offset(30, 5);
		let rect = ratatui::layout::Rect::new(50, 5, 60, 10);
		assert!(s.ensure_visible(rect, (100, 40), (40, 20)));
		assert_eq!(s.view_offset, (50, 5));

		// Oversized rect at origin pulls offset back to 0.
		let mut s = FlowState::new().view_offset(30, 5);
		let rect = ratatui::layout::Rect::new(0, 5, 60, 10);
		assert!(s.ensure_visible(rect, (100, 40), (40, 20)));
		assert_eq!(s.view_offset, (0, 5));
	}
}
