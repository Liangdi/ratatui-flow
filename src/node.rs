use ratatui::{
	style::Style,
	widgets::{Block, BorderType, Borders},
};
use std::collections::HashMap as Map;
use unicode_width::UnicodeWidthStr;

use crate::id::PortId;

/// Render information for a single node.
///
/// `size` is the node's *total* size **including** its 1-cell border, so the
/// inner content area is `(size.0 - 2, size.1 - 2)`. This is the unit that
/// [`crate::NodeGraph`] lays out.
///
/// Optional **port display names** can be attached via
/// [`with_port_name`][Self::with_port_name]; when present, a port's name is
/// drawn one cell *inside* the node from its port symbol (inward of the border).
/// Port *positions* are unchanged — the name is a pure overlay. By default a
/// node has no port names, so rendering is byte-for-byte identical to a node
/// built without them (opt-in).
#[derive(Debug, Clone)]
pub struct NodeLayout<'a> {
	pub size: (u16, u16),
	border_type: BorderType,
	title: &'a str,
	border_style: Style,
	/// Optional display names keyed by [`PortId`]. Drawn one cell inside the
	/// node from the port symbol (Step 9). Defaults to empty — no names → no
	/// rendering change.
	port_names: Map<PortId, &'a str>,
}

impl<'a> NodeLayout<'a> {
	pub fn new(size: (u16, u16)) -> Self {
		Self {
			size,
			border_type: BorderType::Plain,
			title: "",
			border_style: Style::default(),
			port_names: Map::new(),
		}
	}

	/// Build a node whose size auto-fits the given multi-line `content`.
	///
	/// The inner content area is sized to the widest line (by display width)
	/// and the number of lines; the returned `size` then adds 2 cells per axis
	/// for the border, so the content fits exactly inside when rendered into
	/// the rect returned by [`crate::NodeGraph::split`].
	///
	/// Only the *size* is derived from `content` — the node does not store or
	/// render it. Render your own widget into the content rect yourself.
	///
	/// Display width uses `unicode-width`, so wide (e.g. CJK) and zero-width
	/// characters are measured correctly.
	pub fn from_content(content: &str) -> Self {
		let inner_w = content.lines().map(UnicodeWidthStr::width).max().unwrap_or(0);
		let inner_h = content.lines().count();
		Self::new((inner_w.saturating_add(2) as u16, inner_h.saturating_add(2) as u16))
	}

	pub fn with_title(mut self, title: &'a str) -> Self {
		self.title = title;
		self
	}

	pub fn title(&self) -> &str {
		self.title
	}

	pub fn with_border_type(mut self, border: BorderType) -> Self {
		self.border_type = border;
		self
	}

	pub fn border_type(&self) -> BorderType {
		self.border_type
	}

	pub fn with_border_style(mut self, style: Style) -> Self {
		self.border_style = style;
		self
	}

	/// Attach a **display name** to a port (Step 9). When the graph renders a
	/// port symbol (on the node's border), the name's first characters are
	/// written starting one cell *inside* the node from that symbol, going
	/// horizontally and truncated to the available inner width on that row.
	///
	/// Port *positions* are not affected — the [`PortId`] still maps to the same
	/// y/x offset along the node's side (see [`crate::NodeGraph`]). This is a
	/// pure visual overlay: a node with no port names renders byte-for-byte
	/// identically to one without the feature (the field defaults to empty).
	///
	/// Names are best kept short (a few chars); long names get truncated to fit
	/// the node's inner width. The name may collide visually with the node's own
	/// content (which you render separately into the content rect), so prefer
	/// short, distinctive labels.
	///
	/// `port` is matched against a connection's `to_port` (in port, drawn on the
	/// left/top side) or `from_port` (out port, drawn on the right/bottom side)
	/// depending on which connection references it.
	#[must_use]
	pub fn with_port_name(mut self, port: PortId, name: &'a str) -> Self {
		self.port_names.insert(port, name);
		self
	}

	/// The display name attached to `port`, if any (set via
	/// [`with_port_name`][Self::with_port_name]). `None` for ports with no name
	/// (the default).
	pub fn port_name(&self, port: PortId) -> Option<&str> {
		self.port_names.get(&port).copied()
	}

	pub fn block(&self) -> Block<'_> {
		Block::default()
			.borders(Borders::ALL)
			.border_type(self.border_type)
			.border_style(self.border_style)
			.title(self.title)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn from_content_single_line() {
		// "hello" is 5 cells wide; +2 for borders on each axis, 1 line => +2 height.
		let node = NodeLayout::from_content("hello");
		assert_eq!(node.size, (7, 3));
	}

	#[test]
	fn from_content_multi_line_width_is_widest() {
		// widest line is "longer_line" = 11; 3 lines => height 3 + 2 = 5.
		let node = NodeLayout::from_content("short\nlonger_line\ncat");
		assert_eq!(node.size, ((11 + 2), (3 + 2)));
	}

	#[test]
	fn from_content_empty_string() {
		// "" has display width 0; `"".lines().count()` == 0 (no lines), so
		// size = (0+2, 0+2). `.max()` on an empty iterator falls back to 0.
		let node = NodeLayout::from_content("");
		assert_eq!(node.size, (2, 2));
	}

	#[test]
	fn from_content_single_char() {
		let node = NodeLayout::from_content("x");
		assert_eq!(node.size, ((1 + 2), (1 + 2)));
	}

	#[test]
	fn from_content_trailing_newline_does_not_add_line() {
		// `str::lines` does NOT yield a trailing empty element: "a\n" -> ["a"].
		// So this is 1 line, width 1 — documents that `from_content` follows
		// `str::lines` semantics (no phantom trailing line).
		let node = NodeLayout::from_content("a\n");
		assert_eq!(node.size, ((1 + 2), (1 + 2)));
	}

	#[test]
	fn from_content_cjk_uses_display_width() {
		// "中文" is 2 code points but 4 display cells wide (each CJK char is wide).
		// This proves unicode-width is applied, not char count (which would give 2).
		let node = NodeLayout::from_content("中文");
		assert_eq!(node.size, ((4 + 2), (1 + 2)));
	}

	#[test]
	fn from_content_cjk_mixed_with_ascii() {
		// "中" (width 2) + "ab" (width 2) on line 1 = width 4; line 2 "abc" = width 3.
		let node = NodeLayout::from_content("中ab\nabc");
		assert_eq!(node.size, ((4 + 2), (2 + 2)));
	}

	#[test]
	fn from_content_zero_width_chars_dont_add_width() {
		// Combining mark U+0308 has zero display width; "a" + combining diaeresis
		// displays as 1 cell. Width should be 1, not 2 (char count).
		let node = NodeLayout::from_content("a\u{0308}");
		assert_eq!(node.size, ((1 + 2), (1 + 2)));
	}

	#[test]
	fn new_stores_size_directly() {
		let node = NodeLayout::new((10, 4));
		assert_eq!(node.size, (10, 4));
	}

	#[test]
	fn port_name_defaults_to_none() {
		// A fresh node has no port names — opt-in feature.
		let node = NodeLayout::new((6, 4));
		assert_eq!(node.port_name(PortId(0)), None);
		assert_eq!(node.port_name(PortId(7)), None);
	}

	#[test]
	fn with_port_name_sets_and_reads_back() {
		let node = NodeLayout::new((6, 4)).with_port_name(PortId(0), "data");
		assert_eq!(node.port_name(PortId(0)), Some("data"));
		assert_eq!(node.port_name(PortId(1)), None);
	}

	#[test]
	fn with_port_name_overwrites_existing() {
		// setting the same port twice keeps the latest name.
		let node = NodeLayout::new((6, 4))
			.with_port_name(PortId(2), "old")
			.with_port_name(PortId(2), "new");
		assert_eq!(node.port_name(PortId(2)), Some("new"));
	}

	#[test]
	fn node_layout_clones_with_port_names() {
		// adding a field must not break Clone (HashMap<&'a str> is Clone).
		let node = NodeLayout::new((6, 4))
			.with_port_name(PortId(0), "in")
			.with_port_name(PortId(1), "out");
		let cloned = node.clone();
		assert_eq!(cloned.port_name(PortId(0)), Some("in"));
		assert_eq!(cloned.port_name(PortId(1)), Some("out"));
	}
}
