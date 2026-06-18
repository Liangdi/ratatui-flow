//! Integration tests for the text/ASCII export API on `NodeGraph`:
//! `to_ascii` (skeleton only) and `to_ascii_with` (skeleton + node content).
//! These mirror the style of `tests/api.rs` (same `c` / `nid` helpers) and are
//! read-only w.r.t. `src/`.

use std::collections::HashMap as Map;

use ratatui_flow::{Connection, NodeGraph, NodeId, NodeLayout};

/// Shorthand for `Connection::new` (matches `tests/api.rs`): terse callsites.
fn c(from: usize, from_port: usize, to: usize, to_port: usize) -> Connection<'static> {
	Connection::new(from.into(), from_port.into(), to.into(), to_port.into())
}

/// Shorthand for building a [`NodeId`] from a `usize` (matches `tests/api.rs`).
fn nid(n: usize) -> NodeId {
	n.into()
}

/// Build a small deterministic 2-node graph with one connection, sized so the
/// layout fits comfortably in the canvas. Reused by every test below.
fn build_graph() -> NodeGraph<'static> {
	// Node 0 is the root (never a `from_node`); node 1 is its child via the
	// single connection 1 -> 0. Sizes are auto-fit from content so the interior
	// is exactly the content's (width, height).
	let nodes = vec![
		NodeLayout::from_content("ROOT"), // interior 4x1 -> frame 6x3
		NodeLayout::from_content("child"), // interior 5x1 -> frame 7x3
	];
	let conns = vec![c(1, 0, 0, 0)]; // 1 -> 0
	NodeGraph::new(nodes, conns, 40, 12)
}

// ===========================================================================
// to_ascii
// ===========================================================================

/// After `calculate`, `to_ascii` returns a non-empty string with exactly
/// `height` lines (one per canvas row) and contains box-drawing glyphs from the
/// rendered node borders.
#[test]
fn to_ascii_has_height_lines_and_box_glyphs() {
	let mut graph = build_graph();
	graph.calculate();
	let ascii = graph.to_ascii();

	assert!(!ascii.is_empty(), "ascii output non-empty after calculate");
	let lines: Vec<&str> = ascii.split('\n').collect();
	// Exactly `height` rows (canvas is 40x12).
	assert_eq!(lines.len(), 12, "one line per canvas row");
	// Every row has the canvas width (no trimming — full-grid fidelity).
	for (i, line) in lines.iter().enumerate() {
		assert_eq!(
			line.chars().count(),
			40,
			"row {i} is full canvas width (trailing spaces kept)"
		);
	}
	// The skeleton must contain at least one box-drawing border glyph. Rounded
	// corners (╭╮╰╯) come from ratatui's default Plain border on these nodes;
	// vertical/horizontal bars are always present. Asserting on a vertical bar
	// is the most stable across border-type defaults.
	assert!(
		ascii.contains('│') || ascii.contains('─'),
		"ascii contains a box-drawing glyph from a node border"
	);
}

/// Before `calculate`, the off-screen canvas is blank, so `to_ascii` yields
/// `height` lines of all-spaces (defensive — documented behavior, not an error).
#[test]
fn to_ascii_before_calculate_is_all_spaces() {
	// Build but do NOT call calculate.
	let graph = build_graph();
	let ascii = graph.to_ascii();

	assert!(ascii.chars().all(|c| c == ' ' || c == '\n'), "all cells blank");
	let lines: Vec<&str> = ascii.split('\n').collect();
	assert_eq!(lines.len(), 12, "still height rows before calculate");
}

// ===========================================================================
// to_ascii_with
// ===========================================================================

/// `to_ascii_with` overlays node content into each node's interior on top of
/// the skeleton. The distinctive body text of a node must appear inside that
/// node's frame.
#[test]
fn to_ascii_with_overlays_node_content() {
	let mut graph = build_graph();
	graph.calculate();

	let mut bodies: Map<NodeId, &'static str> = Map::new();
	bodies.insert(nid(0), "ROOT");
	bodies.insert(nid(1), "child");
	let ascii = graph.to_ascii_with(|id| bodies.get(&id).copied());

	// Both bodies must appear in the output (overlaid into interiors).
	assert!(ascii.contains("ROOT"), "node 0 body text overlaid into its interior");
	assert!(ascii.contains("child"), "node 1 body text overlaid into its interior");

	// The body must land INSIDE node 0's interior, not on its border. Find the
	// row containing "ROOT" and confirm it's strictly between the node's top
	// and bottom border rows.
	let rect0 = graph.node_rect(nid(0)).expect("node 0 placed");
	let top = rect0.y as usize;
	let bottom = rect0.bottom() as usize; // exclusive
	let root_row = ascii
		.split('\n')
		.enumerate()
		.find(|(_, line)| line.contains("ROOT"))
		.map(|(i, _)| i)
		.expect("ROOT line present");
	assert!(
		top < root_row && root_row < bottom.saturating_sub(1),
		"ROOT on interior row {root_row} (top={top}, bottom={bottom})"
	);
}

/// `to_ascii_with` with a `content` closure that always returns `None` must
/// equal `to_ascii` exactly (skeleton only, no content stamped).
#[test]
fn to_ascii_with_none_content_equals_skeleton() {
	let mut graph = build_graph();
	graph.calculate();

	let skeleton = graph.to_ascii();
	let with_none = graph.to_ascii_with(|_| None);
	assert_eq!(skeleton, with_none, "None content leaves skeleton untouched");
}

/// Content wider than the node's interior is truncated by display width, not
/// by char/byte count. A body line longer than the interior width must not
/// overflow the node's right border.
#[test]
fn to_ascii_with_truncates_long_content_to_interior_width() {
	// Node 0 interior is 4 cells wide (from_content("ROOT") -> frame 6, inner 4).
	// Feed it an 8-char body; only the first 4 chars should land inside.
	let mut graph = build_graph();
	graph.calculate();

	let long_body = "ABCDEFGH";
	let ascii = graph.to_ascii_with(move |_| Some(long_body));

	// The node is 6 wide (frame); the interior is 4 cells. "ABCDEFGH" (8 cells)
	// must be truncated to at most 4 cells, so "EFGH" must NOT appear, and the
	// full 8-char run must not be present.
	assert!(!ascii.contains("EFGH"), "long content truncated to interior width");
	assert!(ascii.contains("ABCD"), "truncated content (ABCD) is present");
}

/// Content is stamped using the CANVAS-space (direction-mirrored) rect, not the
/// raw placement. Under the default `Rtl` direction the main (horizontal) axis
/// is mirrored, so a naive placement-rect overlay would drop content outside its
/// frame. This locks in that the body's COLUMN lands strictly inside the node's
/// canvas-rect interior — the regression that once slipped past the
/// `.contains()`-only checks above.
#[test]
fn to_ascii_with_content_column_inside_canvas_rect_under_rtl() {
	let mut graph = build_graph();
	graph.calculate();
	// Default direction is Rtl — the canvas mirrors the main axis.

	let mut bodies: Map<NodeId, &'static str> = Map::new();
	bodies.insert(nid(0), "ROOT");
	bodies.insert(nid(1), "child");
	let ascii = graph.to_ascii_with(|id| bodies.get(&id).copied());

	for (id, body) in [(nid(0), "ROOT"), (nid(1), "child")] {
		let rect = graph
			.node_canvas_rect(id)
			.unwrap_or_else(|| panic!("node {id:?} has a canvas rect"));
		// Interior column range (canvas space): [rect.x+1, rect.right-1).
		let interior_start = rect.x as usize + 1;
		let interior_end = rect.right() as usize - 1;

		// Find the row carrying this body and locate the body's first column.
		// NOTE: `str::find` returns a BYTE offset, but box-drawing border glyphs
		// are multi-byte UTF-8 (e.g. '│' = 3 bytes), so we must convert to a
		// CHAR index to compare against canvas columns.
		let (row_idx, col) = ascii
			.split('\n')
			.enumerate()
			.find_map(|(i, line)| {
				line.find(body).map(|byte_pos| (i, line[..byte_pos].chars().count()))
			})
			.unwrap_or_else(|| panic!("body {body:?} of node {id:?} present in output"));
		// The row must also be an interior row (top+1 .. bottom-1).
		let top = rect.y as usize;
		let bottom = rect.bottom() as usize;
		assert!(
			top < row_idx && row_idx < bottom.saturating_sub(1),
			"node {id:?} body on interior row {row_idx} (top={top}, bottom={bottom})"
		);
		assert!(
			col >= interior_start && col < interior_end,
			"node {id:?} body column {col} inside interior [{interior_start}, {interior_end}) \
			 (canvas rect x={} right={})",
			rect.x,
			rect.right()
		);
	}
}

/// Wide (CJK) characters are measured by display width, so a 2-cell-wide char
/// counts as 2 columns when fitting into the interior — proving
/// `unicode-width` is applied (mirrors `NodeLayout::from_content` semantics).
#[test]
fn to_ascii_with_measures_cjk_by_display_width() {
	// Build a node whose interior is exactly 4 cells (frame 6). The body "中文字"
	// is 6 display cells (3 CJK chars x 2); only the first two CJK chars (4
	// cells) should fit — the third must be dropped, not half-overwritten.
	let nodes = vec![
		NodeLayout::from_content("abcd"), // interior 4x1 -> frame 6x3 (root)
		NodeLayout::from_content("xy"),   // interior 2x1 -> frame 4x3 (child)
	];
	let conns = vec![c(1, 0, 0, 0)];
	let mut graph = NodeGraph::new(nodes, conns, 30, 10);
	graph.calculate();

	let ascii = graph.to_ascii_with(|_| Some("中文字"));
	// "中" and "文" (4 cells) fit; "字" (would make 6) is dropped.
	assert!(ascii.contains('中'), "first CJK char fits");
	assert!(ascii.contains('文'), "second CJK char fits (4 cells exactly)");
	assert!(
		!ascii.contains('字'),
		"third CJK char dropped (would exceed interior width)"
	);
}
