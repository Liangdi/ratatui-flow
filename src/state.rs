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
}
