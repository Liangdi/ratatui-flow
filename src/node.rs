use ratatui::{
	style::Style,
	widgets::{Block, BorderType, Borders},
};
use unicode_width::UnicodeWidthStr;

/// Render information for a single node.
///
/// `size` is the node's *total* size **including** its 1-cell border, so the
/// inner content area is `(size.0 - 2, size.1 - 2)`. This is the unit that
/// [`crate::NodeGraph`] lays out.
#[derive(Debug)]
pub struct NodeLayout<'a> {
	pub size: (u16, u16),
	border_type: BorderType,
	title: &'a str,
	border_style: Style,
	//	in_ports: Vec<PortLayout>,
	//	out_ports: Vec<PortLayout>,
}

impl<'a> NodeLayout<'a> {
	pub fn new(size: (u16, u16)) -> Self {
		Self {
			size,
			border_type: BorderType::Plain,
			title: "",
			border_style: Style::default(),
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
}
