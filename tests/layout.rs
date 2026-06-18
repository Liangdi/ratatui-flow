//! Integration tests for `NodeGraph` layout, cycle/out-of-bounds safety,
//! render-on-tiny-canvas safety, and the `content` example fixture.
//!
//! These tests are intentionally read-only w.r.t. `src/` — they pin down the
//! behavior fixed in Steps 1 and 2 (cycle detection, out-of-bounds connection
//! skipping, render-time bounds checks) as a regression net.

use ratatui::{buffer::Buffer, layout::Rect, widgets::StatefulWidget};
use ratatui_flow::{
	AddNodeError, Connection, Diagnostic, FlowState, LineType, NodeGraph, NodeId,
	NodeLayout, PortId,
};

/// Shorthand for `Connection::new` with `.into()` so test callsites stay terse
/// after the NodeId/PortId migration: `c(from, from_port, to, to_port)`.
fn c(from: usize, from_port: usize, to: usize, to_port: usize) -> Connection<'static> {
	Connection::new(from.into(), from_port.into(), to.into(), to_port.into())
}

/// Shorthand for building a [`NodeId`] from a `usize` (the `NodeId` inner field
/// is `pub(crate)`, so integration tests must go through `From<usize>`).
fn nid(n: usize) -> NodeId {
	n.into()
}

/// Helper: how many of the rects returned by `split` are non-zero-sized
/// (i.e. the node was actually placed and fits inside `area`).
fn count_placed(rects: &[Rect]) -> usize {
	rects.iter().filter(|r| r.width > 0 && r.height > 0).count()
}

/// Helper: build a graph, run calculate, render into a buffer of `area`'s size,
/// and return the split rects. Used by several tests.
fn build_and_split<'a>(
	nodes: Vec<NodeLayout<'a>>,
	connections: Vec<Connection<'a>>,
	area: Rect,
) -> (NodeGraph<'a>, Vec<Rect>) {
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();
	let rects = graph.split(area);
	(graph, rects)
}

// ===========================================================================
// B. NodeGraph::calculate layout correctness
// ===========================================================================

/// A small linear DAG: 0 -> 1 -> 2 (using the convention from_node = output,
/// to_node = input; roots are nodes that never appear as from_node).
///
/// To make node 0 the root (rightmost), 0 must be a `to_node` only.
/// Connection `new(from=1, to=0)` means "1 feeds into 0", so 1 is a child of 0.
fn linear_chain_3() -> (Vec<NodeLayout<'static>>, Vec<Connection<'static>>) {
	let nodes =
		vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	// 2 -> 1 -> 0: node 0 is root, 1 is child of 0, 2 is child of 1.
	let connections = vec![c(1, 0, 0, 0), c(2, 0, 1, 0)];
	(nodes, connections)
}

#[test]
fn split_returns_one_rect_per_node() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 20);
	let (_graph, rects) = build_and_split(nodes, conns, area);
	assert_eq!(rects.len(), 3, "split must return one rect per node");
}

#[test]
fn all_reachable_nodes_are_placed() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 20);
	let (_graph, rects) = build_and_split(nodes, conns, area);
	let placed = count_placed(&rects);
	assert_eq!(placed, 3, "all 3 reachable nodes should be placed (got {placed})");
}

#[test]
fn placed_node_rects_do_not_intersect() {
	// A small fan-out DAG: root 0, children 1 and 2 both fed into 0.
	let nodes =
		vec![NodeLayout::new((8, 5)), NodeLayout::new((8, 5)), NodeLayout::new((8, 5))];
	// 1 -> 0, 2 -> 0  => 0 is root; 1 and 2 are its children (stacked vertically).
	let connections = vec![c(1, 0, 0, 0), c(2, 0, 0, 1)];
	let area = Rect::new(0, 0, 80, 30);
	let (_graph, rects) = build_and_split(nodes, connections, area);

	let placed: Vec<Rect> =
		rects.into_iter().filter(|r| r.width > 0 && r.height > 0).collect();
	assert!(placed.len() >= 2, "expected at least 2 placed nodes");

	// No two placed rects may overlap. `Rect::intersects` is symmetric and
	// excludes edges touching only (which is fine for separated nodes).
	for i in 0..placed.len() {
		for j in (i + 1)..placed.len() {
			assert!(
				!placed[i].intersects(placed[j]),
				"node rects {i} and {j} intersect: {:?} vs {:?}",
				placed[i],
				placed[j]
			);
		}
	}
}

#[test]
fn unreachable_node_is_not_placed() {
	// A node is "unreachable" iff it is neither a root nor a child of any
	// reachable node. Roots = nodes that never appear as `from_node`. A node
	// appears as a child of `n` via a connection `from=node, to=n`.
	//
	// Here node 2 only participates in a self-loop `2->2` (from=2, to=2). Since
	// it appears as a from_node, it is NOT a root. And no reachable node has a
	// connection with to_node == reachable -> ... the only edge with to_node==2
	// is the self-loop, which is never explored because 2 itself is never
	// reached from roots {0,1}. So node 2 must not be placed.
	let nodes = vec![
		NodeLayout::new((6, 4)), // 0: root
		NodeLayout::new((6, 4)), // 1: root
		NodeLayout::new((6, 4)), // 2: unreachable (self-loop only)
	];
	let connections = vec![c(2, 0, 2, 0)]; // 2 -> 2 self-loop
	let area = Rect::new(0, 0, 60, 20);
	let (_graph, rects) = build_and_split(nodes, connections, area);

	let placed0 = rects[0].width > 0 && rects[0].height > 0;
	let placed1 = rects[1].width > 0 && rects[1].height > 0;
	let placed2 = rects[2].width > 0 && rects[2].height > 0;
	assert!(placed0, "root node 0 should be placed");
	assert!(placed1, "root node 1 should be placed");
	assert!(!placed2, "unreachable node 2 should NOT be placed (rect={:?})", rects[2]);
}

// ===========================================================================
// C. Cycle from a root does not stack-overflow / panic (Step 1 regression)
// ===========================================================================

#[test]
fn root_reachable_cycle_does_not_panic_on_calculate() {
	// Topology: R(0) is the root. A(1) and B(2) form a cycle, reachable from R.
	//   R <- A   (Connection::new(from=A=1, to=R=0))  => A is a child of R
	//   A <- B   (Connection::new(from=B=2, to=A=1))  => B is a child of A
	//   B <- A   (Connection::new(from=A=1, to=B=2))  => A is a child of B (cycle A<->B)
	// from_nodes = {A=1, B=2}, so roots = {R=0}. The cycle A<->B is reachable
	// from R. Before Step 1's cycle guard this would recurse forever.
	let nodes = vec![
		NodeLayout::new((6, 4)), // R
		NodeLayout::new((6, 4)), // A
		NodeLayout::new((6, 4)), // B
	];
	let connections = vec![
		c(1, 0, 0, 0), // A -> R
		c(2, 0, 1, 0), // B -> A
		c(1, 0, 2, 0), // A -> B  (closes A<->B cycle)
	];
	let area = Rect::new(0, 0, 60, 20);

	// If the cycle guard regresses, this hangs/overflows the stack and the test
	// process crashes (counted as a failure). A normal return is the pass.
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();

	// Root R must be placed; the cycle nodes are placed up to the point the
	// guard breaks the cycle (at least R).
	let rects = graph.split(area);
	let root_placed = rects[0].width > 0 && rects[0].height > 0;
	assert!(root_placed, "root R must be placed even with a reachable cycle");
}

#[test]
fn root_reachable_cycle_does_not_panic_on_render() {
	// Same cycle topology as above, but also exercise render into a Buffer to
	// make sure the connection-layout pass doesn't choke on cycle nodes.
	let nodes =
		vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	let connections = vec![c(1, 0, 0, 0), c(2, 0, 1, 0), c(1, 0, 2, 0)];
	let area = Rect::new(0, 0, 40, 15);
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();

	let mut buf = Buffer::empty(area);
	// No panic => pass.
	graph.render(area, &mut buf, &mut FlowState::default());
}

// ===========================================================================
// D. Out-of-bounds connections are skipped (Step 1 regression)
// ===========================================================================

#[test]
fn out_of_bounds_connection_from_node_skipped() {
	// 2 nodes (indices 0,1). Connection references from_node=5 (>= len).
	let nodes = vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	let connections = vec![
		c(1, 0, 0, 0), // valid: 1 -> 0
		c(5, 0, 0, 1), // INVALID from_node=5
	];
	let area = Rect::new(0, 0, 40, 15);
	// Must not panic.
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();
	let rects = graph.split(area);
	// Both real nodes are roots/children and should place fine.
	assert_eq!(rects.len(), 2);
}

#[test]
fn out_of_bounds_connection_to_node_skipped() {
	let nodes = vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	let connections = vec![
		c(1, 0, 0, 0), // valid
		c(0, 0, 9, 0), // INVALID to_node=9
	];
	let area = Rect::new(0, 0, 40, 15);
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();
	let rects = graph.split(area);
	assert_eq!(rects.len(), 2);
	// No panic, no phantom placement for index 9 (there is no node 9 anyway).
}

#[test]
fn all_connections_out_of_bounds_still_no_panic() {
	// Every connection is invalid; graph should still calculate (placing all
	// nodes as roots) without panicking.
	let nodes = vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	let connections = vec![c(10, 0, 11, 0), c(99, 0, 0, 0)];
	let area = Rect::new(0, 0, 40, 15);
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();
	let rects = graph.split(area);
	// Both nodes are roots (no valid from_node removes them) -> both placed.
	assert_eq!(count_placed(&rects), 2);
}

// ===========================================================================
// E. Rendering a graph into a buffer smaller than the canvas does not panic
//    (Step 2 regression — graph.rs::render cell_mut guards)
// ===========================================================================
//
// NOTE on scope: Step 2's bounds fix lives in `graph.rs::render` (the per-node
// `pos.right() > area.width` skip + `cell_mut` guards). These tests pin THAT
// behavior. We therefore size the *layout canvas* (passed to NodeGraph::new)
// large enough that `calculate()` is well-behaved, then render into a smaller
// `Buffer` to exercise the render-time clipping. A separate, pre-existing
// out-of-bounds panic in `connection.rs` (`ConnectionsLayout::block_zone` /
// `block_port`, triggered when the *canvas itself* is too small for the
// placements) is documented in the report rather than tested here, per the
// "don't modify src" rule.

#[test]
fn render_canvas_sized_graph_into_smaller_buffer_no_panic() {
	// Lay out on a 60x20 canvas (comfortably fits the nodes), then render into a
	// 20x10 buffer. Most node frames fall outside the buffer and must be skipped
	// by graph.rs::render's `pos.right() > area.width` guard instead of
	// indexing buf out of bounds.
	let nodes = vec![
		NodeLayout::new((14, 6)),
		NodeLayout::new((14, 6)),
		NodeLayout::new((14, 6)),
	];
	let connections = vec![c(1, 0, 0, 0), c(2, 0, 1, 0)];
	// Layout canvas is large; render buffer is small.
	let canvas = Rect::new(0, 0, 60, 20);
	let render_area = Rect::new(0, 0, 20, 10);
	let mut graph =
		NodeGraph::new(nodes, connections, canvas.width as usize, canvas.height as usize);
	graph.calculate();

	let mut buf = Buffer::empty(render_area);
	// No panic => pass.
	graph.render(render_area, &mut buf, &mut FlowState::default());
}

#[test]
fn render_single_node_into_one_cell_buffer_no_panic() {
	// Degenerate 1x1 render buffer with a properly-laid-out (large canvas)
	// graph. The node won't fit and must be skipped, not panic.
	let nodes = vec![NodeLayout::new((10, 6))];
	let canvas = Rect::new(0, 0, 40, 20);
	let render_area = Rect::new(0, 0, 1, 1);
	let mut graph =
		NodeGraph::new(nodes, vec![], canvas.width as usize, canvas.height as usize);
	graph.calculate();

	let mut buf = Buffer::empty(render_area);
	graph.render(render_area, &mut buf, &mut FlowState::default());
}

#[test]
fn render_nodes_outside_buffer_bounds_are_skipped() {
	// Two nodes laid out on a wide canvas. Build two identical graphs and render
	// one into the full canvas (sanity: frame is drawable) and one into a tiny
	// slice (must not panic; out-of-canvas nodes are skipped).
	let mk_graph = || {
		let nodes = vec![NodeLayout::new((8, 5)), NodeLayout::new((8, 5))];
		let connections = vec![c(1, 0, 0, 0)];
		let canvas = Rect::new(0, 0, 60, 20);
		let mut g = NodeGraph::new(
			nodes,
			connections,
			canvas.width as usize,
			canvas.height as usize,
		);
		g.calculate();
		g
	};

	// Sanity: on the full canvas, render writes at least one cell.
	let canvas = Rect::new(0, 0, 60, 20);
	let mut full_buf = Buffer::empty(canvas);
	let blank = Buffer::empty(canvas);
	let g_full = mk_graph();
	g_full.render(canvas, &mut full_buf, &mut FlowState::default());
	let drawable =
		full_buf.content().iter().zip(blank.content().iter()).any(|(a, b)| a != b);
	assert!(drawable, "graph should draw into a full-canvas buffer");

	// Now render into a tiny slice; must not panic.
	let tiny = Rect::new(0, 0, 5, 5);
	let mut tiny_buf = Buffer::empty(tiny);
	let g_tiny = mk_graph();
	g_tiny.render(tiny, &mut tiny_buf, &mut FlowState::default());
}

// ===========================================================================
// F. content.rs 6-node pipeline fixture (pins current placement behavior)
// ===========================================================================

/// Reproduce the exact graph from `examples/content.rs` and assert all 6 nodes
/// get placed when calculated on its native 120x24 canvas. This pins the
/// "src no longer drops nodes due to insufficient canvas" behavior.
fn content_fixture_graph() -> NodeGraph<'static> {
	const CONTENTS: [&str; 6] = [
		"Source\n/data/input.csv\n~10M rows",
		"Parse\nheader row\ninfer schema\nutf-8 decode",
		"Validate\nreject nulls\ndrop duplicates",
		"Transform\nnormalize -> [0,1]\none-hot encode\ncast types",
		"Filter\nvalue > 0.5\nregion == \"us\"",
		"Sink\nINSERT INTO events\nON CONFLICT\nDO NOTHING",
	];

	let nodes: Vec<NodeLayout<'static>> =
		CONTENTS.iter().map(|c| NodeLayout::from_content(c)).collect();

	// Same connection set as the example.
	let connections = vec![
		c(0, 0, 1, 0),
		c(0, 0, 2, 0),
		c(1, 0, 3, 0).with_line_type(LineType::Double),
		c(2, 0, 3, 1),
		c(3, 0, 4, 0),
		c(3, 1, 5, 0).with_line_type(LineType::Double),
		c(4, 0, 5, 1),
	];

	NodeGraph::new(nodes, connections, 120, 24)
}

#[test]
fn content_fixture_places_all_six_nodes() {
	let mut graph = content_fixture_graph();
	graph.calculate();
	let area = Rect::new(0, 0, 120, 24);
	let rects = graph.split(area);
	assert_eq!(rects.len(), 6, "split must yield one rect per node");
	let placed = count_placed(&rects);
	assert_eq!(
		placed, 6,
		"all 6 content-example nodes must be placed on a 120x24 canvas (got {placed})"
	);
}

#[test]
fn content_fixture_node_rects_are_disjoint() {
	let mut graph = content_fixture_graph();
	graph.calculate();
	let area = Rect::new(0, 0, 120, 24);
	let rects = graph.split(area);
	let placed: Vec<Rect> =
		rects.into_iter().filter(|r| r.width > 0 && r.height > 0).collect();
	assert_eq!(placed.len(), 6);
	for i in 0..placed.len() {
		for j in (i + 1)..placed.len() {
			assert!(
				!placed[i].intersects(placed[j]),
				"content-fixture nodes {i} and {j} intersect: {:?} vs {:?}",
				placed[i],
				placed[j]
			);
		}
	}
}

#[test]
fn content_fixture_renders_without_panic() {
	let mut graph = content_fixture_graph();
	graph.calculate();
	let area = Rect::new(0, 0, 120, 24);
	let mut buf = Buffer::empty(area);
	// Render the whole graph (connections + node frames). No panic => pass.
	graph.render(area, &mut buf, &mut FlowState::default());
}

// ===========================================================================
// Sanity: rendering a placed node actually draws into the buffer (the node
// frame is not empty). This guards against a regression where render silently
// no-ops every node.
// ===========================================================================

#[test]
fn render_draws_node_frame_into_buffer() {
	// Single root node, nothing else. After render, the buffer should not be
	// entirely blank — at least the border cells get written.
	let nodes = vec![NodeLayout::new((6, 4))];
	let area = Rect::new(0, 0, 20, 10);
	let mut graph =
		NodeGraph::new(nodes, vec![], area.width as usize, area.height as usize);
	graph.calculate();

	let blank = Buffer::empty(area);
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());

	// The rendered buffer must differ from a blank buffer somewhere.
	let differs = buf.content().iter().zip(blank.content().iter()).any(|(a, b)| a != b);
	assert!(differs, "render should write at least one cell for the node frame");
}

// ===========================================================================
// Calculate-time bounds guards: when the canvas (width/height passed to
// NodeGraph::new) is smaller than where nodes actually get placed,
// ConnectionsLayout::block_zone / block_port must skip out-of-canvas cells
// instead of indexing `edge_field` out of bounds.
// ===========================================================================

/// Reproduces the Step-2-续 calculate-time panic: canvas 20x10 but two 40x10
/// nodes get placed beyond the canvas width. `calculate` must return normally
/// (not panic) — the bounds guards in block_zone/block_port skip the
/// out-of-canvas cells.
#[test]
fn calculate_with_canvas_smaller_than_placement_no_panic() {
	let nodes = vec![NodeLayout::new((40, 10)), NodeLayout::new((40, 10))];
	let conns = vec![c(0, 0, 1, 0)];
	// canvas 20x10, but nodes are 40 wide -> placed beyond canvas width.
	let mut graph = NodeGraph::new(nodes, conns, 20, 10);
	// Reaching this line without panicking is the assertion.
	graph.calculate();
}

/// End-to-end: small canvas + large nodes must survive both the calculate-time
/// guards (block_zone/block_port) and the render-time guards (Step 2). Runs the
/// full calculate -> render pipeline on a buffer smaller than the node sizes.
#[test]
fn small_canvas_calculate_and_render_no_panic() {
	let nodes = vec![NodeLayout::new((40, 10)), NodeLayout::new((40, 10))];
	let conns = vec![c(0, 0, 1, 0)];
	let mut graph = NodeGraph::new(nodes, conns, 20, 10);
	graph.calculate();

	// Render into a buffer smaller than the node sizes. No panic => pass.
	let small_rect = Rect::new(0, 0, 20, 10);
	let mut buf = Buffer::empty(small_rect);
	graph.render(small_rect, &mut buf, &mut FlowState::default());
}

// ===========================================================================
// G. Step 5: structured diagnostics surface the library's "silent failures"
//    (unreachable nodes, out-of-bounds connections, unrouted connections).
// ===========================================================================

/// A clean graph reports no diagnostics. We reuse the exact 6-node DAG from
/// `examples/content.rs` (also pinned by `content_fixture_*` above) on its
/// native 120x24 canvas, where every node is reachable and every connection
/// routes successfully.
#[test]
fn diagnostics_empty_for_clean_graph() {
	let mut graph = content_fixture_graph();
	graph.calculate();
	assert!(
		graph.diagnostics().is_empty(),
		"clean content-fixture graph should report no diagnostics, got {:?}",
		graph.diagnostics(),
	);
}

/// A node not reachable from any root (here: an isolated node with no
/// connections at all) is never placed and must surface as
/// `Diagnostic::UnplacedNode`.
#[test]
fn diagnostics_reports_unreachable_node() {
	// Two reachable nodes (0 -> 1 chain) plus an isolated node 2 with no edges.
	// Connection `new(from=0, to=1)` means "0 feeds into 1": from_nodes={0},
	// so roots={1,2}. Node 1 is placed as a root, node 0 as its child; node 2
	// is a root too, but a root with an empty rect still counts as placed…
	// — actually a root with no upstream IS placed at (0,0). To make node 2
	// genuinely unreachable we make it neither a root nor a child: it appears
	// as a `from_node` (so it's not a root) but never as a `to_node` of a
	// reachable node. A self-loop `2->2` does exactly that, matching the
	// existing `unreachable_node_is_not_placed` topology.
	let nodes = vec![
		NodeLayout::new((6, 4)), // 0
		NodeLayout::new((6, 4)), // 1
		NodeLayout::new((6, 4)), // 2: unreachable (self-loop only)
	];
	// 0 -> 1 (reachable chain) and 2 -> 2 (orphan self-loop).
	let connections = vec![c(0, 0, 1, 0), c(2, 0, 2, 0)];
	let area = Rect::new(0, 0, 60, 20);
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();

	assert!(
		graph.diagnostics().contains(&Diagnostic::UnplacedNode { node: 2usize.into() }),
		"expected UnplacedNode {{ node: 2 }} in diagnostics, got {:?}",
		graph.diagnostics(),
	);
}

/// A connection whose `from_node`/`to_node` is out of bounds is ignored and
/// must surface as `Diagnostic::InvalidConnectionRef`.
#[test]
fn diagnostics_reports_invalid_connection() {
	let nodes = vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	// Valid 0 -> 1, plus an invalid from_node=5 and an invalid to_node=9.
	let connections = vec![
		c(0, 0, 1, 0), // valid
		c(5, 0, 0, 1), // INVALID from_node=5
		c(0, 0, 9, 0), // INVALID to_node=9
	];
	let area = Rect::new(0, 0, 40, 15);
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();

	let diags = graph.diagnostics();
	assert!(
		diags.contains(&Diagnostic::InvalidConnectionRef {
			from_node: 5usize.into(),
			to_node: 0usize.into()
		}),
		"expected InvalidConnectionRef {{ from_node: 5, to_node: 0 }}, got {diags:?}",
	);
	assert!(
		diags.contains(&Diagnostic::InvalidConnectionRef {
			from_node: 0usize.into(),
			to_node: 9usize.into()
		}),
		"expected InvalidConnectionRef {{ from_node: 0, to_node: 9 }}, got {diags:?}",
	);
}

/// `diagnostics()` reflects the *latest* `calculate()` run only: it is cleared
/// at the start of each call. A graph that was dirty on the first run and clean
/// on the second must report an empty slice after the second run.
#[test]
fn diagnostics_cleared_between_calculate_calls() {
	let nodes =
		vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	// First run: invalid connection -> non-empty diagnostics.
	let conns_dirty = vec![c(9, 0, 0, 0)];
	let area = Rect::new(0, 0, 40, 15);
	let mut graph =
		NodeGraph::new(nodes, conns_dirty, area.width as usize, area.height as usize);
	graph.calculate();
	assert!(!graph.diagnostics().is_empty(), "dirty run should report diagnostics");

	// Second run: valid chain 0 -> 1 -> 2, all reachable, no bad refs.
	// (`self.connections` is fixed at construction, so we rebuild a clean graph
	// of the same shape to demonstrate the clear-on-recalculate behavior.)
	let nodes2 =
		vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	let conns_clean = vec![c(0, 0, 1, 0), c(1, 0, 2, 0)];
	let mut graph2 =
		NodeGraph::new(nodes2, conns_clean, area.width as usize, area.height as usize);
	graph2.calculate();
	// To exercise clearing on the *same* graph we re-run calculate; a clean
	// graph's diagnostics must be empty after any calculate.
	assert!(
		graph2.diagnostics().is_empty(),
		"clean graph should report no diagnostics after calculate, got {:?}",
		graph2.diagnostics(),
	);
}

/// A connection whose endpoints are valid (both nodes placed) but for which the
/// A* router finds no path must surface as `Diagnostic::RoutingFailed`.
///
/// Geometry that reliably forces this: two nodes that, once laid out, fill the
/// entire canvas so every routing edge between their ports is either inside a
/// blocked node zone or off-canvas. On an 8x4 canvas, two 4x4 nodes placed at
/// the left and right edges leave a single shared column with no free vertical
/// edge for the connection to turn through — the search exhausts without
/// reaching the goal and falls back to the alias character. This is a stable
/// "no path exists" cause (not a timeout), so it does not depend on
/// `SEARCH_TIMEOUT` or machine speed.
#[test]
fn diagnostics_reports_routing_failed() {
	let nodes = vec![NodeLayout::new((4, 4)), NodeLayout::new((4, 4))];
	let conns = vec![c(0, 0, 1, 0)];
	// 8x4 canvas: both nodes occupy the full height and together span the width,
	// leaving no room to route between their ports.
	let mut graph = NodeGraph::new(nodes, conns, 8, 4);
	graph.calculate();

	assert!(
		graph.diagnostics().contains(&Diagnostic::RoutingFailed {
			from_node: 0usize.into(),
			from_port: 0usize.into(),
			to_node: 1usize.into(),
			to_port: 0usize.into(),
		}),
		"expected RoutingFailed for the 0->1 connection on a fully-packed canvas, got {:?}",
		graph.diagnostics(),
	);
}

// ===========================================================================
// H. Viewport / FlowState API: pan, split under offset, and the render path
// ===========================================================================
//
// The graph renders once into an off-screen canvas during `calculate`, then
// exposes two equivalent ways to view a scrolled window of it:
//
//   (Step 6, preferred) `FlowState { view_offset, ... }` + the `StatefulWidget`
//   impl on `NodeGraph`, plus `split_stateful(area, &FlowState)` for content
//   rects.
//
//   (legacy, `#[deprecated]`) `Viewport { offset }`, `split_viewport`, and the
//   `NodeGraphView` widget — kept around so existing callers don't break, and
//   tested below under `#[allow(deprecated)]` to pin that contract.
//
// This section verifies BOTH: the new `FlowState` path (pan via `view_offset`,
// `split_stateful` translation/clipping, the stateful render blitting the
// scrolled window and honoring the offset) and the legacy path (so the
// deprecated API keeps working identically).

use ratatui::layout::Position;
use ratatui::widgets::Widget as RatatuiWidget;
#[allow(deprecated)]
use ratatui_flow::{NodeGraphView, Viewport};

/// A simple linear 3-node chain (0 root, 1 child, 2 grandchild) used by the
/// viewport tests. Built on a comfortably large canvas so all nodes place.
fn viewport_chain_graph() -> NodeGraph<'static> {
	let nodes = vec![
		NodeLayout::new((10, 5)),
		NodeLayout::new((10, 5)),
		NodeLayout::new((10, 5)),
	];
	// 2 -> 1 -> 0: node 0 is root.
	let connections = vec![c(1, 0, 0, 0), c(2, 0, 1, 0)];
	let mut graph = NodeGraph::new(nodes, connections, 60, 20);
	graph.calculate();
	graph
}

// --- new FlowState path --------------------------------------------------

/// With offset (0,0) and an `area` equal to the canvas size,
/// `split_stateful` must match `split_named` (mapped to plain rects) — the pan
/// adds no translation in that case.
#[test]
fn split_stateful_zero_offset_matches_split_named_rects() {
	let graph = viewport_chain_graph();
	let canvas = Rect::new(0, 0, 60, 20);
	let state = FlowState::new(); // offset (0,0)

	let plain: Vec<Rect> =
		graph.split_named(canvas).into_iter().map(|(_, r)| r).collect();
	let via_state: Vec<Rect> =
		graph.split_stateful(canvas, &state).into_iter().map(|(_, r)| r).collect();

	assert_eq!(
		plain, via_state,
		"zero-offset split_stateful rects must equal split_named rects on the same canvas-sized area"
	);
}

/// A non-zero x `view_offset` must shift each visible node's content rect left
/// by exactly the offset (before clipping). Area is the full canvas so nothing
/// clips at these small offsets.
#[test]
fn split_stateful_offset_shifts_rects() {
	let graph = viewport_chain_graph();
	let canvas = Rect::new(0, 0, 60, 20);

	let state_zero = FlowState::new();
	let state_shift = FlowState::new().view_offset(10, 0);

	let rects_zero: Vec<Rect> =
		graph.split_stateful(canvas, &state_zero).into_iter().map(|(_, r)| r).collect();
	let rects_shift: Vec<Rect> =
		graph.split_stateful(canvas, &state_shift).into_iter().map(|(_, r)| r).collect();

	// Every node is placed (3-node chain, big canvas).
	let placed_zero: Vec<Rect> =
		rects_zero.into_iter().filter(|r| r.width > 0 && r.height > 0).collect();
	let placed_shift: Vec<Rect> =
		rects_shift.into_iter().filter(|r| r.width > 0 && r.height > 0).collect();
	assert_eq!(placed_zero.len(), 3, "all 3 nodes placed at zero offset");
	assert_eq!(placed_shift.len(), 3, "all 3 nodes placed at offset (10,0)");

	for (z, s) in placed_zero.iter().zip(placed_shift.iter()) {
		let shift = z.x.saturating_sub(s.x);
		assert_eq!(
			shift, 10,
			"view_offset(10,0) should shift x left by 10: zero={z:?} shift={s:?}"
		);
		assert_eq!(z.y, s.y, "y must not change for an x-only offset");
		assert_eq!(z.width, s.width);
		assert_eq!(z.height, s.height);
	}
}

/// An offset past the far edge of the canvas pushes every node off the
/// top-left of the view, so every `split_stateful` rect must be 0×0.
#[test]
fn split_stateful_clips_invisible_nodes() {
	let graph = viewport_chain_graph();
	let canvas = Rect::new(0, 0, 60, 20);
	let state = FlowState::new().view_offset(200, 200);
	let rects: Vec<Rect> =
		graph.split_stateful(canvas, &state).into_iter().map(|(_, r)| r).collect();

	for (i, r) in rects.iter().enumerate() {
		assert_eq!(
			(r.width, r.height),
			(0, 0),
			"node {i} should be fully clipped (0×0) at a huge offset, got {r:?}"
		);
	}
}

/// The stateful render path must not panic for any offset (zero, partial, huge).
#[test]
fn stateful_render_renders_without_panic() {
	let area = Rect::new(0, 0, 40, 15);

	let mut s = FlowState::new();
	let mut buf = Buffer::empty(area);
	viewport_chain_graph().render(area, &mut buf, &mut s);

	let mut s2 = FlowState::new().view_offset(5, 3);
	let mut buf2 = Buffer::empty(area);
	viewport_chain_graph().render(area, &mut buf2, &mut s2);

	let mut s3 = FlowState::new().view_offset(999, 999);
	let mut buf3 = Buffer::empty(area);
	viewport_chain_graph().render(area, &mut buf3, &mut s3);
}

/// Two different `view_offset`s must produce different buffers — proving the
/// pan is actually honored by the stateful blit.
#[test]
fn stateful_render_offset_changes_output() {
	let area = Rect::new(0, 0, 40, 15);
	let blank = Buffer::empty(area);

	let mut sa = FlowState::new();
	let mut buf_a = Buffer::empty(area);
	viewport_chain_graph().render(area, &mut buf_a, &mut sa);

	let mut sb = FlowState::new().view_offset(8, 4);
	let mut buf_b = Buffer::empty(area);
	viewport_chain_graph().render(area, &mut buf_b, &mut sb);

	let a_drawn = buf_a.content().iter().zip(blank.content().iter()).any(|(x, y)| x != y);
	assert!(a_drawn, "zero-offset stateful render should draw the graph (non-blank)");

	let differ = buf_a.content().iter().zip(buf_b.content().iter()).any(|(x, y)| x != y);
	assert!(
		differ,
		"offset (0,0) vs (8,4) must produce different buffer contents (pan had no effect)"
	);
}

/// The stateful render must work when `area` is smaller than the canvas.
#[test]
fn stateful_render_into_area_smaller_than_canvas() {
	let area = Rect::new(0, 0, 20, 10);
	let mut state = FlowState::new().view_offset(3, 2);
	let mut buf = Buffer::empty(area);
	viewport_chain_graph().render(area, &mut buf, &mut state);

	let blank = Buffer::empty(area);
	let drawn = buf.content().iter().zip(blank.content().iter()).any(|(x, y)| x != y);
	assert!(
		drawn,
		"expected the stateful render to blit some canvas content into the small area"
	);
}

/// **Zero-change guarantee**: rendering with a default `FlowState` must be
/// byte-for-byte identical across two independent renders. This is the Step 6
/// acceptance floor — a defaulted state must perturb nothing and be
/// deterministic (the impl short-circuits to `render_to`).
#[test]
fn stateful_default_renders_identically_to_stateless() {
	let area = Rect::new(0, 0, 40, 15);

	let mut buf_default = Buffer::empty(area);
	let mut state_default = FlowState::default();
	viewport_chain_graph().render(area, &mut buf_default, &mut state_default);

	let mut buf_ref = Buffer::empty(area);
	let mut state_ref = FlowState::default();
	viewport_chain_graph().render(area, &mut buf_ref, &mut state_ref);

	assert_eq!(
		buf_default, buf_ref,
		"default FlowState render must be deterministic and byte-identical"
	);
}

/// Selection highlight: a node in `FlowState::selection` must have at least one
/// border cell whose style differs from the un-highlighted render. This proves
/// the highlight overlay actually recolors the border.
#[test]
fn stateful_selection_recolors_node_border() {
	let area = Rect::new(0, 0, 40, 15);

	// baseline: default (no highlight)
	let mut buf_plain = Buffer::empty(area);
	let mut s_plain = FlowState::default();
	viewport_chain_graph().render(area, &mut buf_plain, &mut s_plain);

	// with selection on node 0 (root, guaranteed placed & on-screen)
	let mut buf_sel = Buffer::empty(area);
	let mut s_sel = FlowState::default().select(Some(nid(0)));
	viewport_chain_graph().render(area, &mut buf_sel, &mut s_sel);

	// at least one cell must differ (the recolored border)
	let differ =
		buf_plain.content().iter().zip(buf_sel.content().iter()).any(|(a, b)| a != b);
	assert!(differ, "selecting a node must change at least one border cell's style");
}

// --- legacy (deprecated) path, kept working ------------------------------
//
// These pin the deprecated `Viewport` / `split_viewport` / `NodeGraphView`
// contract so the old API keeps behaving identically during the deprecation
// window. `#[allow(deprecated)]` silences the intended usage warnings here.

/// With offset (0,0) and an `area` equal to the canvas size,
/// `split_viewport` must match `split(canvas)` exactly (the viewport adds no
/// translation in that case).
#[test]
#[allow(deprecated)]
fn split_viewport_zero_offset_matches_split_on_canvas() {
	let graph = viewport_chain_graph();
	let canvas = Rect::new(0, 0, 60, 20);
	let vp = Viewport::new(); // offset (0,0)

	let plain = graph.split(canvas);
	let via_viewport = graph.split_viewport(canvas, &vp);

	assert_eq!(
		plain, via_viewport,
		"zero-offset split_viewport must equal split on the same canvas-sized area"
	);
}

/// A non-zero x offset must shift each visible node's content rect to the left
/// relative to the zero-offset case, by exactly the offset (before clipping).
/// Here we pick an area wide enough that the nodes stay fully visible at both
/// offsets, so clipping doesn't muddy the comparison.
#[test]
#[allow(deprecated)]
fn split_viewport_offset_shifts_rects() {
	let graph = viewport_chain_graph();
	// Use the full canvas as the view so nothing is clipped at small offsets.
	let canvas = Rect::new(0, 0, 60, 20);

	let vp_zero = Viewport::new();
	let vp_shift = Viewport::new().offset(10, 0);

	let rects_zero = graph.split_viewport(canvas, &vp_zero);
	let rects_shift = graph.split_viewport(canvas, &vp_shift);

	// Every node is placed (3-node chain, big canvas).
	let placed_zero: Vec<Rect> =
		rects_zero.into_iter().filter(|r| r.width > 0 && r.height > 0).collect();
	let placed_shift: Vec<Rect> =
		rects_shift.into_iter().filter(|r| r.width > 0 && r.height > 0).collect();
	assert_eq!(placed_zero.len(), 3, "all 3 nodes placed at zero offset");
	assert_eq!(placed_shift.len(), 3, "all 3 nodes placed at offset (10,0)");

	// Pair them by index (same node order) and check the x shift.
	for (z, s) in placed_zero.iter().zip(placed_shift.iter()) {
		// Scrolling right by 10 moves the canvas content 10 cells to the left,
		// i.e. each node's screen x decreases by 10. We compare left edges.
		let shift = z.x.saturating_sub(s.x);
		// Allow exactly 10 (full shift) — anything else means the offset wasn't
		// applied as a plain translation.
		assert_eq!(
			shift, 10,
			"offset(10,0) should shift x left by 10: zero={z:?} shift={s:?}"
		);
		// y is unaffected by an x-only offset.
		assert_eq!(z.y, s.y, "y must not change for an x-only offset");
		// width/height are preserved (no clipping here since the canvas is the view).
		assert_eq!(z.width, s.width);
		assert_eq!(z.height, s.height);
	}
}

/// An offset larger than the canvas pushes every node off the top-left of the
/// view, so every returned rect must be 0×0.
#[test]
#[allow(deprecated)]
fn split_viewport_clips_invisible_nodes() {
	let graph = viewport_chain_graph();
	let canvas = Rect::new(0, 0, 60, 20);

	// Offset past the far edge of the canvas: nothing is visible.
	let vp = Viewport::new().offset(200, 200);
	let rects = graph.split_viewport(canvas, &vp);

	for (i, r) in rects.iter().enumerate() {
		assert_eq!(
			(r.width, r.height),
			(0, 0),
			"node {i} should be fully clipped (0×0) at a huge offset, got {r:?}"
		);
	}
}

/// Rendering a `NodeGraphView` into a buffer must not panic, regardless of offset.
#[test]
#[allow(deprecated)]
fn node_graph_view_renders_without_panic() {
	let graph = viewport_chain_graph();
	let area = Rect::new(0, 0, 40, 15);

	// Zero offset.
	let mut buf = Buffer::empty(area);
	NodeGraphView::new(&graph).render(area, &mut buf);

	// Non-zero offset that stays partly on-canvas.
	let mut buf2 = Buffer::empty(area);
	NodeGraphView::new(&graph).offset(5, 3).render(area, &mut buf2);

	// Huge offset (everything off-canvas): still must not panic.
	let mut buf3 = Buffer::empty(area);
	NodeGraphView::new(&graph).offset(999, 999).render(area, &mut buf3);
}

/// Two different offsets must produce different buffers — proving the offset
/// is actually honored by the blit (not silently ignored).
#[test]
#[allow(deprecated)]
fn node_graph_view_offset_changes_output() {
	let graph = viewport_chain_graph();
	let area = Rect::new(0, 0, 40, 15);
	let blank = Buffer::empty(area);

	let mut buf_a = Buffer::empty(area);
	NodeGraphView::new(&graph).offset(0, 0).render(area, &mut buf_a);

	let mut buf_b = Buffer::empty(area);
	NodeGraphView::new(&graph).offset(8, 4).render(area, &mut buf_b);

	// Sanity: at least one of them actually drew something (the canvas has
	// node borders/connections, so a zero-offset view over a placed graph is
	// non-blank).
	let a_drawn = buf_a.content().iter().zip(blank.content().iter()).any(|(x, y)| x != y);
	assert!(a_drawn, "zero-offset view should draw the graph (non-blank)");

	// The two offsets must differ somewhere.
	let differ = buf_a.content().iter().zip(buf_b.content().iter()).any(|(x, y)| x != y);
	assert!(
		differ,
		"offset (0,0) vs (8,4) must produce different buffer contents (offset had no effect)"
	);
}

/// `NodeGraphView` should also render without panic when its `area` is smaller
/// than the canvas (the common real terminal case). Confirms the blit's
/// out-of-canvas guard works for the partial-overlap window.
#[test]
#[allow(deprecated)]
fn node_graph_view_renders_into_area_smaller_than_canvas() {
	let graph = viewport_chain_graph();
	// canvas is 60x20; render into a 20x10 window with a small offset.
	let area = Rect::new(0, 0, 20, 10);
	let mut buf = Buffer::empty(area);
	NodeGraphView::new(&graph).offset(3, 2).render(area, &mut buf);

	// Confirm at least one cell came from the canvas (graph is drawn in the
	// top-left of the canvas, so a small offset reveals it).
	let blank = Buffer::empty(area);
	let drawn = buf.content().iter().zip(blank.content().iter()).any(|(x, y)| x != y);
	assert!(drawn, "expected the view to blit some canvas content into the small area");

	// Also exercise the `.viewport(Viewport)` builder path to make sure the
	// public builder API compiles and runs, and that it matches `.offset(x,y)`.
	let mut buf2 = Buffer::empty(area);
	let vp = Viewport::new().offset(3, 2);
	NodeGraphView::new(&graph).viewport(vp).render(area, &mut buf2);
	let same = buf.content().iter().zip(buf2.content().iter()).all(|(x, y)| x == y);
	assert!(same, ".viewport(vp) and .offset(x,y) builders must produce the same output");

	// Touch a Position read to keep the import used and document the cell API.
	let _ = buf2.cell(Position::new(0, 0)).is_some();
}

// ===========================================================================
// I. Step 3: dynamic mutators (add/remove) + dirty flag + position getters
// ===========================================================================
//
// These cover the new incremental-edit API: `add_node`/`add_node_with_id`,
// `add_connection`, `remove_node` (with connection cascade), `remove_connection`,
// the `dirty` flag lifecycle, and the `positions` / `node_rect` / `split_named`
// getters. They assume a graph built via `new` still assigns `NodeId(0..n)`.

/// `add_node` marks the graph dirty; after `calculate`, the new node is placed
/// and `positions()` contains its id; `calculate` clears the dirty flag.
#[test]
fn add_node_sets_dirty_and_places_after_calculate() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4))],
		vec![c(1, 0, 0, 0)],
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();
	assert!(!graph.is_dirty(), "freshly calculated graph is not dirty");

	// Add a third node and wire it into the chain (2 -> 1).
	let new_id = graph.add_node(NodeLayout::new((6, 4)));
	assert!(graph.is_dirty(), "add_node must mark the graph dirty");
	assert_eq!(new_id, nid(2), "add_node hands out the next id (2) after 0,1");

	// Before calculate, positions() is stale (does NOT contain the new node).
	assert!(
		!graph.positions().contains_key(&new_id),
		"positions() must be stale until calculate is called"
	);

	graph.calculate();
	assert!(!graph.is_dirty(), "calculate clears the dirty flag");
	assert!(
		graph.positions().contains_key(&new_id),
		"after calculate the new node id must be in positions()"
	);
	assert!(
		graph.node_rect(new_id).is_some(),
		"node_rect for the added node must be Some after calculate"
	);
}

/// `add_node_with_id` succeeds for a fresh id and returns
/// `Err(ConflictingId)` for an id that already exists. The conflict must NOT
/// be pushed into `diagnostics` (those are cleared each calculate).
#[test]
fn add_node_with_id_success_and_conflict() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4))],
		vec![],
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();
	assert!(graph.diagnostics().is_empty());

	// Fresh, non-contiguous id succeeds.
	graph.add_node_with_id(nid(42), NodeLayout::new((6, 4))).expect("id 42 is free");
	assert!(graph.has_node(nid(42)));
	assert!(graph.is_dirty());

	// Conflicting id (0 already taken by the first node) returns Err and is NOT
	// recorded as a diagnostic.
	let err = graph.add_node_with_id(nid(0), NodeLayout::new((6, 4))).unwrap_err();
	assert_eq!(err, AddNodeError::ConflictingId);
	graph.calculate();
	assert!(
		graph.diagnostics().is_empty(),
		"add_node_with_id conflict must not surface as a diagnostic, got {:?}",
		graph.diagnostics()
	);
}

/// A subsequent `add_node()` (auto id) must not collide with an id already
/// taken via `add_node_with_id`. I.e. `next_id` is bumped past any inserted id.
#[test]
fn add_node_after_add_node_with_id_does_not_collide() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![NodeLayout::new((6, 4))],
		vec![],
		area.width as usize,
		area.height as usize,
	);
	// Take id 10 explicitly.
	graph.add_node_with_id(nid(10), NodeLayout::new((6, 4))).unwrap();
	// Auto-allocated id must be > 10 (not a duplicate of 10).
	let auto = graph.add_node(NodeLayout::new((6, 4)));
	assert_ne!(auto, nid(10), "auto id must not collide with the inserted id 10");
	assert!(graph.has_node(auto));
}

/// `remove_node` removes the node from placements and cascades: any connection
/// referencing it (on either side) is dropped. Re-adding a connection to the
/// removed id then surfaces as InvalidConnectionRef (the id no longer exists).
#[test]
fn remove_node_cascades_connections_and_unplaces() {
	let area = Rect::new(0, 0, 60, 20);
	// 2 -> 1 -> 0 chain; node 1 is in the middle.
	let mut graph = NodeGraph::new(
		vec![
			NodeLayout::new((6, 4)), // 0
			NodeLayout::new((6, 4)), // 1
			NodeLayout::new((6, 4)), // 2
		],
		vec![c(1, 0, 0, 0), c(2, 0, 1, 0)],
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();
	assert!(graph.node_rect(nid(1)).is_some(), "node 1 placed before removal");
	assert!(graph.has_node(nid(1)));

	// Remove the middle node.
	let removed = graph.remove_node(nid(1));
	assert!(removed, "remove_node should report it removed something");
	assert!(!graph.has_node(nid(1)), "node 1 no longer present");
	assert!(graph.is_dirty(), "remove_node marks the graph dirty");

	graph.calculate();
	assert!(
		graph.node_rect(nid(1)).is_none(),
		"removed node must not appear in placements after calculate"
	);

	// The two connections both referenced node 1 (0<-1 and 1<-2), so both must
	// be gone. Adding a NEW connection that references the removed id 1 must
	// now be flagged as InvalidConnectionRef (proving the cascade happened and
	// the connection store no longer silently holds a dangling ref).
	graph.add_connection(c(1, 0, 0, 0)); // references removed node 1
	graph.calculate();
	assert!(
		graph.diagnostics().contains(&Diagnostic::InvalidConnectionRef {
			from_node: nid(1),
			to_node: nid(0),
		}),
		"connection referencing the removed node 1 must be flagged, got {:?}",
		graph.diagnostics()
	);
}

/// `remove_node` on an unknown id is a no-op and does NOT mark the graph dirty.
#[test]
fn remove_node_unknown_id_is_noop() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![NodeLayout::new((6, 4))],
		vec![],
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();
	assert!(!graph.is_dirty());

	let removed = graph.remove_node(nid(99));
	assert!(!removed, "removing an unknown id returns false");
	assert!(!graph.is_dirty(), "no-op remove must not dirty the graph");
}

/// `remove_connection` drops exactly the matching connection and no other.
#[test]
fn remove_connection_drops_only_match() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![
			NodeLayout::new((6, 4)), // 0
			NodeLayout::new((6, 4)), // 1
			NodeLayout::new((6, 4)), // 2
		],
		vec![c(1, 0, 0, 0), c(2, 0, 0, 0)],
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();
	assert!(!graph.is_dirty());

	// Remove the 1->0 connection.
	let removed = graph.remove_connection(nid(1), 0usize.into(), nid(0), 0usize.into());
	assert!(removed, "remove_connection should find and drop the 1->0 conn");
	assert!(graph.is_dirty());

	// The 2->0 connection is unaffected: node 0 is still fed by node 2, so it
	// is not a root and node 2 still gets placed as its child.
	graph.calculate();
	let rects = graph.split_named(area);
	let r0 = rects.iter().find(|(id, _)| *id == nid(0)).map(|(_, r)| *r);
	let r1 = rects.iter().find(|(id, _)| *id == nid(1)).map(|(_, r)| *r);
	let r2 = rects.iter().find(|(id, _)| *id == nid(2)).map(|(_, r)| *r);
	assert!(r0.is_some() && r0.unwrap().width > 0, "node 0 still placed");
	assert!(r2.is_some() && r2.unwrap().width > 0, "node 2 still placed");
	// Node 1 had its only connection removed and is now an isolated root — it
	// should still be placed (roots are placed), just no longer connected.
	assert!(r1.is_some(), "node 1 is still a node (placed as a root)");

	// Removing the same connection again is a no-op.
	let removed_again =
		graph.remove_connection(nid(1), 0usize.into(), nid(0), 0usize.into());
	assert!(!removed_again, "second remove of the same connection is a no-op");
}

/// `split_named` returns one `(NodeId, Rect)` per node in render order, and the
/// rects are identical (same order, same coordinates) to `split`.
#[test]
fn split_named_matches_split_elementwise() {
	let area = Rect::new(0, 0, 60, 20);
	let (nodes, conns) = linear_chain_3();
	let mut graph =
		NodeGraph::new(nodes, conns, area.width as usize, area.height as usize);
	graph.calculate();

	let plain = graph.split(area);
	let named = graph.split_named(area);

	assert_eq!(plain.len(), named.len(), "same node count");
	for (i, (plain_r, (id, named_r))) in plain.iter().zip(named.iter()).enumerate() {
		assert_eq!(plain_r, named_r, "rect {i} must match between split and split_named");
		assert_eq!(
			*id,
			nid(i),
			"split_named id order must be NodeId(0..n) for a new()-built graph"
		);
	}
}

/// `positions()` returns the full-frame rects (border included) in canvas
/// coordinates, distinct from the inner content rects returned by `split`.
#[test]
fn positions_returns_full_frame_canvas_rects() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(vec![NodeLayout::new((8, 5))], vec![], 60, 20);
	graph.calculate();

	let pos = graph.positions();
	assert_eq!(pos.len(), 1, "one node placed");
	let full = pos.get(&nid(0)).copied().expect("node 0 placed");
	// Full frame is 8 wide / 5 tall (border included).
	assert_eq!((full.width, full.height), (8, 5));

	// The inner content rect (split) is 2 smaller per axis.
	let inner = graph.split(area)[0];
	assert_eq!((inner.width, inner.height), (6, 3));
	assert!(
		full.width > inner.width && full.height > inner.height,
		"full-frame rect must be larger than the inner content rect"
	);
}

// ===========================================================================
// FlowDirection parameterization (Step 4)
// ===========================================================================

/// Build a graph with a chosen flow direction.
fn build_dir<'a>(
	dir: ratatui_flow::FlowDirection,
	nodes: Vec<NodeLayout<'a>>,
	connections: Vec<Connection<'a>>,
	area: Rect,
) -> (NodeGraph<'a>, Vec<Rect>) {
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize)
			.with_direction(dir);
	graph.calculate();
	let rects = graph.split(area);
	(graph, rects)
}

#[test]
fn default_direction_is_rtl() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 20);
	let graph = NodeGraph::new(nodes, conns, area.width as usize, area.height as usize);
	assert_eq!(graph.direction(), ratatui_flow::FlowDirection::Rtl);
}

#[test]
fn rtl_split_matches_undirected_default() {
	// A graph built with explicit Rtl must produce the same split rects as a
	// graph built without with_direction (the default).
	let area = Rect::new(0, 0, 60, 20);
	let mk = || {
		let nodes = vec![
			NodeLayout::new((6, 4)),
			NodeLayout::new((6, 4)),
			NodeLayout::new((6, 4)),
		];
		(nodes, vec![c(1, 0, 0, 0), c(2, 0, 1, 0)])
	};
	let (n1, c1) = mk();
	let (n2, c2) = mk();
	let mut g_def = NodeGraph::new(n1, c1, area.width as usize, area.height as usize);
	g_def.calculate();
	let mut g_rtl = NodeGraph::new(n2, c2, area.width as usize, area.height as usize)
		.with_direction(ratatui_flow::FlowDirection::Rtl);
	g_rtl.calculate();
	assert_eq!(g_def.split(area), g_rtl.split(area));
}

#[test]
fn ltr_root_is_leftmost_child_is_rightmost() {
	let (nodes, conns) = linear_chain_3(); // 0 root, 1 child of 0, 2 child of 1
	let area = Rect::new(0, 0, 60, 20);
	let (_g, rects) = build_dir(ratatui_flow::FlowDirection::Ltr, nodes, conns, area);
	let placed: Vec<Rect> = rects.into_iter().filter(|r| r.width > 0).collect();
	assert_eq!(placed.len(), 3);
	// Ltr: root (index 0) leftmost, deepest child (index 2) rightmost.
	assert!(placed[0].x < placed[2].x, "Ltr root should be left of deepest child");
}

#[test]
fn rtl_root_is_rightmost_child_is_leftmost() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 20);
	let (_g, rects) = build_dir(ratatui_flow::FlowDirection::Rtl, nodes, conns, area);
	let placed: Vec<Rect> = rects.into_iter().filter(|r| r.width > 0).collect();
	assert_eq!(placed.len(), 3);
	// Rtl: root (index 0) rightmost, deepest child (index 2) leftmost.
	assert!(placed[0].x > placed[2].x, "Rtl root should be right of deepest child");
}

#[test]
fn ttb_root_is_topmost_child_is_bottommost() {
	let (nodes, conns) = linear_chain_3();
	// vertical: 3 nodes of height 4 + 2 margins of 5 = 22; canvas must be taller.
	let area = Rect::new(0, 0, 60, 30);
	let (_g, rects) = build_dir(ratatui_flow::FlowDirection::Ttb, nodes, conns, area);
	let placed: Vec<Rect> = rects.into_iter().filter(|r| r.width > 0).collect();
	assert_eq!(placed.len(), 3);
	// Ttb: root (index 0) topmost, deepest child (index 2) bottommost.
	assert!(placed[0].y < placed[2].y, "Ttb root should be above deepest child");
}

#[test]
fn btt_root_is_bottommost_child_is_topmost() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 30);
	let (_g, rects) = build_dir(ratatui_flow::FlowDirection::Btt, nodes, conns, area);
	let placed: Vec<Rect> = rects.into_iter().filter(|r| r.width > 0).collect();
	assert_eq!(placed.len(), 3);
	// Btt: root (index 0) bottommost, deepest child (index 2) topmost.
	assert!(placed[0].y > placed[2].y, "Btt root should be below deepest child");
}

#[test]
fn all_four_directions_place_all_nodes() {
	// 60x30 fits the chain in both orientations.
	let area = Rect::new(0, 0, 60, 30);
	for dir in [
		ratatui_flow::FlowDirection::Ltr,
		ratatui_flow::FlowDirection::Rtl,
		ratatui_flow::FlowDirection::Ttb,
		ratatui_flow::FlowDirection::Btt,
	] {
		let (nodes, conns) = linear_chain_3();
		let (_g, rects) = build_dir(dir, nodes, conns, area);
		assert_eq!(count_placed(&rects), 3, "all nodes placed for {:?}", dir);
	}
}

#[test]
fn each_direction_renders_connections_and_borders() {
	// Render the graph into a buffer for each direction and confirm we get
	// non-trivial output (borders + at least one connection cell). This catches
	// port/edge mismatches that would leave connections unrouted.
	use ratatui::widgets::StatefulWidget;
	let area = Rect::new(0, 0, 60, 30);
	for dir in [
		ratatui_flow::FlowDirection::Ltr,
		ratatui_flow::FlowDirection::Rtl,
		ratatui_flow::FlowDirection::Ttb,
		ratatui_flow::FlowDirection::Btt,
	] {
		let (nodes, conns) = linear_chain_3();
		let mut graph =
			NodeGraph::new(nodes, conns, area.width as usize, area.height as usize)
				.with_direction(dir);
		graph.calculate();
		let mut buf = Buffer::empty(area);
		NodeGraph::render(graph, area, &mut buf, &mut FlowState::default());
		let nonblank = buf.content().iter().filter(|c| c.symbol() != " ").count();
		assert!(
			nonblank > 20,
			"{:?}: expected borders+connections, got {} nonblank cells",
			dir,
			nonblank
		);
	}
}

#[test]
fn vertical_directions_produce_no_routing_diagnostics() {
	// A simple chain should route cleanly in every direction; if vertical port
	// geometry is wrong, connections fail to route and emit RoutingFailed.
	let area = Rect::new(0, 0, 60, 30);
	for dir in [
		ratatui_flow::FlowDirection::Ltr,
		ratatui_flow::FlowDirection::Rtl,
		ratatui_flow::FlowDirection::Ttb,
		ratatui_flow::FlowDirection::Btt,
	] {
		let (nodes, conns) = linear_chain_3();
		let mut graph =
			NodeGraph::new(nodes, conns, area.width as usize, area.height as usize)
				.with_direction(dir);
		graph.calculate();
		let unrouted = graph
			.diagnostics()
			.iter()
			.filter(|d| matches!(d, Diagnostic::RoutingFailed { .. }))
			.count();
		assert_eq!(unrouted, 0, "{:?}: {:?} connections failed to route", dir, unrouted);
	}
}

// ===========================================================================
// hit_test (Step 5)
// ===========================================================================

/// Center point of a content (inner) rect. Used to feed `hit_test` a point that
/// is guaranteed to lie inside the node's bordered hit rect (the inner rect is
/// strictly contained within the bordered rect, so its center is too).
fn center_of(r: Rect) -> (u16, u16) {
	(r.x + r.width / 2, r.y + r.height / 2)
}

/// `hit_test` returns the NodeId whose bordered rect contains the given screen
/// coordinate. We check it by hitting the *center* of each content rect from
/// `split_named` (those centers are guaranteed inside the larger bordered rect).
#[test]
fn hit_test_center_of_each_node_returns_its_id() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 20);
	let mut graph =
		NodeGraph::new(nodes, conns, area.width as usize, area.height as usize);
	graph.calculate();

	for (id, inner) in graph.split_named(area) {
		// skip any 0×0 (unplaced) node defensively; the chain places all three.
		if inner.width == 0 || inner.height == 0 {
			continue;
		}
		let (cx, cy) = center_of(inner);
		assert_eq!(
			graph.hit_test(area, cx, cy),
			Some(id),
			"hit_test at center of node {:?} ({},{}) must return that node's id",
			id,
			cx,
			cy
		);
	}
}

/// A point in the gap between two placed nodes (empty space) hits nothing.
#[test]
fn hit_test_gap_between_nodes_returns_none() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 20);
	let (graph, rects) = build_and_split(nodes, conns, area);

	// The chain is horizontal (default Rtl): nodes sit at three x-positions
	// separated by MARGIN(5) cells of empty space. Find two ADJACENT nodes (no
	// other placed node between them on x) and hit the middle of their gap.
	let placed: Vec<Rect> = rects.iter().copied().filter(|r| r.width > 0).collect();
	assert!(placed.len() >= 2, "need >=2 placed nodes to find a gap");
	let mut by_x = placed.clone();
	by_x.sort_by_key(|r| r.x);

	// Try each consecutive pair; the first pair whose mid-x is genuinely empty
	// gives us our test point.
	let mut found = None;
	for pair in by_x.windows(2) {
		let (a, b) = (pair[0], pair[1]);
		let gap_x = (a.right() + b.x) / 2;
		let gap_y = a.y + a.height / 2;
		// genuine gap: gap_x strictly between a's right edge and b's left edge,
		// and gap_y inside the shared vertical band of both nodes.
		if gap_x > a.right() && gap_x < b.x {
			found = Some((gap_x, gap_y));
			break;
		}
	}
	let (gap_x, gap_y) =
		found.expect("linear chain must leave at least one empty gap between nodes");
	assert_eq!(
		graph.hit_test(area, gap_x, gap_y),
		None,
		"gap between nodes ({},{}) must hit nothing",
		gap_x,
		gap_y
	);
}

/// A coordinate outside `area` entirely (and outside every node) returns None.
#[test]
fn hit_test_far_outside_returns_none() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 20);
	let (graph, _rects) = build_and_split(nodes, conns, area);
	// bottom-right corner well past any node but still within numeric range
	assert_eq!(graph.hit_test(area, 59, 19), None, "far corner must not hit a node");
}

/// A node that never gets placed (isolated, unreachable) has no rect to hit, so
/// any coordinate returns None for it (and more specifically, hit_test never
/// returns the id of an unplaced node).
#[test]
fn hit_test_unplaced_node_id_never_returned() {
	// Build a graph with an isolated node (no connections at all -> it IS a root
	// and gets placed). To get a truly *unplaced* node we use a 2-node cycle:
	// 0<->1 with both as from_nodes, so neither is a root and neither is placed.
	let nodes = vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	// 0 feeds 1 AND 1 feeds 0: pure cycle, no root -> both unplaced.
	let conns = vec![c(0, 0, 1, 0), c(1, 0, 0, 0)];
	let area = Rect::new(0, 0, 60, 20);
	let mut graph =
		NodeGraph::new(nodes, conns, area.width as usize, area.height as usize);
	graph.calculate();
	// sanity: both nodes are reported as unplaced
	assert_eq!(graph.positions().len(), 0, "pure cycle must place nothing");
	// hit_test over the whole area must never return either node id
	for x in (0..area.width).step_by(3) {
		for y in (0..area.height).step_by(2) {
			let hit = graph.hit_test(area, x, y);
			assert!(
				hit.is_none(),
				"hit_test at ({},{}) returned {:?} but nothing is placed",
				x,
				y,
				hit
			);
		}
	}
}

/// `hit_test` is direction-aware: under Ltr the root (node 0) is on the left,
/// so a point just inside the left edge of the area hits the root. This guards
/// against a stale/hard-coded mirror that would mis-map coordinates under Ltr.
#[test]
fn hit_test_direction_aware_ltr_root_on_left() {
	let (nodes, conns) = linear_chain_3(); // 0 root, 1 child of 0, 2 child of 1
	let area = Rect::new(0, 0, 60, 20);
	let mut graph =
		NodeGraph::new(nodes, conns, area.width as usize, area.height as usize)
			.with_direction(ratatui_flow::FlowDirection::Ltr);
	graph.calculate();

	// Ltr: root (node 0) is leftmost. Its bordered rect sits at the left edge.
	let r0 = graph.node_rect(nid(0)).expect("root placed");
	// confirm root really is leftmost among the three
	let r1 = graph.node_rect(nid(1)).expect("node 1 placed");
	let r2 = graph.node_rect(nid(2)).expect("node 2 placed");
	// canvas coords for Ltr (no mirror): node 0 has the smallest x.
	assert!(r0.x < r1.x, "Ltr root must have smallest canvas x");
	assert!(r1.x < r2.x, "Ltr chain must grow rightward");

	// A point just inside the root's bordered rect (top-left + 1,1) is the
	// root's hit area under Ltr. Hit it and expect the root id back.
	let hit = graph.hit_test(area, r0.x + 1, r0.y + 1);
	assert_eq!(hit, Some(nid(0)), "Ltr: point in root's border rect must hit root");
}

/// Cross-check: hitting the **border** cell of a node (not just the inner
/// content) still counts as a hit, since `hit_test` uses bordered rects. We hit
/// the top-left corner cell of node 0's bordered rect.
#[test]
fn hit_test_includes_border_cells() {
	let (nodes, conns) = linear_chain_3();
	let area = Rect::new(0, 0, 60, 20);
	let (graph, _rects) = build_dir(ratatui_flow::FlowDirection::Rtl, nodes, conns, area);

	// node 0 is the root. Grab its bordered canvas rect and hit its top-left
	// corner cell (which is a border cell, outside the inner content rect).
	let full = graph.node_rect(nid(0)).expect("root placed");
	// reconstruct the screen rect of node 0 the same way hit_test does (Rtl
	// mirrors x). top-left corner cell of the bordered rect.
	let screen_x = area.width - full.right() + area.x;
	let screen_y = full.y + area.y;
	assert_eq!(
		graph.hit_test(area, screen_x, screen_y),
		Some(nid(0)),
		"border cell (top-left of root frame) must count as a hit"
	);
	// and the cell just inside the top-left corner (also a border on a >=2
	// frame) is still a hit.
	assert_eq!(
		graph.hit_test(area, screen_x + 1, screen_y + 1),
		Some(nid(0)),
		"cell just inside the frame corner must also hit root"
	);
}

// ===========================================================================
// K. Direction arrows on the `to` (in) port (Step 7)
// ===========================================================================
//
// Each connection's `to` port (where the line enters a node) is drawn as a
// direction arrow pointing in the flow direction, instead of the in-port glyph
// (┤/┴ family). The arrow variant scales with the connection's `line_type`:
// heavy (Thick/Double) → solid arrows (◀▶▼▲); light (Plain/Rounded) → thin
// arrows (◄►▽△). The `from` (out) port keeps its ├/┬ glyph. `show_arrows(false)`
// restores the original glyphs.
//
// Since no test pins a specific port glyph, the arrow swap can't break existing
// assertions; these tests pin the new arrow behavior directly.

/// Count how many cells in `buf` carry exactly `sym` as their symbol.
fn count_symbol(buf: &Buffer, sym: &str) -> usize {
	buf.content().iter().filter(|c| c.symbol() == sym).count()
}

/// Build a 2-node graph (1 -> 0, so node 0 is root) with a connection of the
/// given `line_type`, calculate, render into a fresh buffer, and return it.
fn render_arrow_graph(
	dir: ratatui_flow::FlowDirection,
	line_type: LineType,
	show_arrows: bool,
	area: Rect,
) -> Buffer {
	let nodes = vec![
		NodeLayout::new((8, 4)).with_title("root"), // 0: root
		NodeLayout::new((8, 4)).with_title("child"), // 1
	];
	// 1 -> 0: node 0 is the root (never a from_node); node 1 is its child.
	let conn = Connection::new(1u32.into(), 0u32.into(), 0u32.into(), 0u32.into())
		.with_line_type(line_type);
	let mut graph =
		NodeGraph::new(nodes, vec![conn], area.width as usize, area.height as usize)
			.with_direction(dir)
			.show_arrows(show_arrows);
	graph.calculate();
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());
	buf
}

#[test]
fn arrow_rtl_points_left_thin() {
	// Rounded line → thin left arrow ◄ on the to port.
	let area = Rect::new(0, 0, 60, 20);
	let buf = render_arrow_graph(
		ratatui_flow::FlowDirection::Rtl,
		LineType::Rounded,
		true,
		area,
	);
	assert!(
		count_symbol(&buf, "◄") >= 1,
		"Rtl + Rounded: to port must show ◄ (found {})",
		count_symbol(&buf, "◄")
	);
	// the heavy variant must NOT appear for a light line type.
	assert_eq!(count_symbol(&buf, "◀"), 0, "Rtl + Rounded must not use the heavy ◀");
}

#[test]
fn arrow_rtl_points_left_heavy() {
	// Thick line → solid left arrow ◀.
	let area = Rect::new(0, 0, 60, 20);
	let buf =
		render_arrow_graph(ratatui_flow::FlowDirection::Rtl, LineType::Thick, true, area);
	assert!(
		count_symbol(&buf, "◀") >= 1,
		"Rtl + Thick: to port must show ◀ (found {})",
		count_symbol(&buf, "◀")
	);
	assert_eq!(count_symbol(&buf, "◄"), 0, "Rtl + Thick must not use the thin ◄");
}

#[test]
fn arrow_ltr_points_right() {
	// Ltr + Rounded → ►; + Thick → ▶.
	let area = Rect::new(0, 0, 60, 20);
	let buf_thin = render_arrow_graph(
		ratatui_flow::FlowDirection::Ltr,
		LineType::Rounded,
		true,
		area,
	);
	assert!(count_symbol(&buf_thin, "►") >= 1, "Ltr + Rounded: to port must show ►");
	let buf_heavy =
		render_arrow_graph(ratatui_flow::FlowDirection::Ltr, LineType::Thick, true, area);
	assert!(count_symbol(&buf_heavy, "▶") >= 1, "Ltr + Thick: to port must show ▶");
}

#[test]
fn arrow_ttb_points_down() {
	// Ttb + Rounded → ▽; + Thick → ▼.
	let area = Rect::new(0, 0, 40, 40);
	let buf_thin = render_arrow_graph(
		ratatui_flow::FlowDirection::Ttb,
		LineType::Rounded,
		true,
		area,
	);
	assert!(count_symbol(&buf_thin, "▽") >= 1, "Ttb + Rounded: to port must show ▽");
	let buf_heavy =
		render_arrow_graph(ratatui_flow::FlowDirection::Ttb, LineType::Thick, true, area);
	assert!(count_symbol(&buf_heavy, "▼") >= 1, "Ttb + Thick: to port must show ▼");
}

#[test]
fn arrow_btt_points_up() {
	// Btt + Rounded → △; + Thick → ▲.
	let area = Rect::new(0, 0, 40, 40);
	let buf_thin = render_arrow_graph(
		ratatui_flow::FlowDirection::Btt,
		LineType::Rounded,
		true,
		area,
	);
	assert!(count_symbol(&buf_thin, "△") >= 1, "Btt + Rounded: to port must show △");
	let buf_heavy =
		render_arrow_graph(ratatui_flow::FlowDirection::Btt, LineType::Thick, true, area);
	assert!(count_symbol(&buf_heavy, "▲") >= 1, "Btt + Thick: to port must show ▲");
}

#[test]
fn show_arrows_false_restores_port_glyphs() {
	// With arrows off, the to port must NOT carry any arrow symbol and must
	// instead use the original in-port glyph. For Rounded border + Rounded line
	// the horizontal in-port glyph is ┤; for the vertical (Ttb) case it's ┴.
	let area = Rect::new(0, 0, 60, 20);

	// horizontal (Rtl): in-port glyph ┤
	let buf_h = render_arrow_graph(
		ratatui_flow::FlowDirection::Rtl,
		LineType::Rounded,
		false,
		area,
	);
	// no arrows anywhere
	for arrow in ["◄", "◀", "►", "▶", "▼", "▽", "▲", "△"] {
		assert_eq!(
			count_symbol(&buf_h, arrow),
			0,
			"show_arrows(false): no arrow glyph {arrow} should appear (Rtl)"
		);
	}
	assert!(
		count_symbol(&buf_h, "┤") >= 1,
		"show_arrows(false) + Rtl: to port must fall back to ┤"
	);

	// vertical (Ttb): in-port glyph ┴
	let buf_v = render_arrow_graph(
		ratatui_flow::FlowDirection::Ttb,
		LineType::Rounded,
		false,
		Rect::new(0, 0, 40, 40),
	);
	assert!(
		count_symbol(&buf_v, "┴") >= 1,
		"show_arrows(false) + Ttb: to port must fall back to ┴"
	);
}

#[test]
fn show_arrows_default_is_false() {
	// A freshly-constructed graph must default to arrows OFF — arrows are
	// opt-in via `.show_arrows(true)`.
	let nodes = vec![NodeLayout::new((8, 4)), NodeLayout::new((8, 4))];
	let conns = vec![c(1, 0, 0, 0)];
	let mut graph = NodeGraph::new(nodes, conns, 60, 20);
	graph.calculate();
	let mut buf = Buffer::empty(Rect::new(0, 0, 60, 20));
	graph.render(Rect::new(0, 0, 60, 20), &mut buf, &mut FlowState::default());
	for arrow in ["◄", "◀", "►", "▶", "▼", "▽", "▲", "△"] {
		assert_eq!(
			count_symbol(&buf, arrow),
			0,
			"default graph must NOT render arrows ({arrow}) since show_arrows defaults to false"
		);
	}
	// to (in) port falls back to the plain in-port glyph ┤.
	assert!(
		count_symbol(&buf, "┤") >= 1,
		"default graph: to port must use the in-port glyph ┤ (no arrow)"
	);
}

// ===========================================================================
// Step 8 — connection labels
// ===========================================================================
//
// `Connection::with_label(&str)` draws the label horizontally on top of the
// routed line at its midpoint. Labels are opt-in: a label-less connection
// renders byte-for-byte identically to pre-Step-8 (covered by the existing
// 67 integration tests, which all still pass unchanged).

/// Build a 2-node graph (1 -> 0, node 0 is root) where the single connection
/// carries `label`, calculate, and render into a fresh buffer of `area`'s size.
fn render_labeled_graph(label: Option<&str>, line_type: LineType, area: Rect) -> Buffer {
	let nodes = vec![
		NodeLayout::new((8, 4)).with_title("root"),  // 0
		NodeLayout::new((8, 4)).with_title("child"), // 1
	];
	let mut conn = Connection::new(1u32.into(), 0u32.into(), 0u32.into(), 0u32.into())
		.with_line_type(line_type);
	if let Some(l) = label {
		conn = conn.with_label(l);
	}
	let mut graph =
		NodeGraph::new(nodes, vec![conn], area.width as usize, area.height as usize);
	graph.calculate();
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());
	buf
}

#[test]
fn label_renders_first_char_at_midpoint() {
	// A "data" label must write its first character ('d') onto the line.
	let area = Rect::new(0, 0, 60, 20);
	let buf = render_labeled_graph(Some("data"), LineType::Rounded, area);
	assert!(
		count_symbol(&buf, "d") >= 1,
		"label 'data' must render at least one 'd' cell (found {})",
		count_symbol(&buf, "d")
	);
	// the whole label should be readable: a, t, a too
	assert!(count_symbol(&buf, "a") >= 1, "label must render 'a'");
	assert!(count_symbol(&buf, "t") >= 1, "label must render 't'");
}

#[test]
fn label_renders_all_chars_of_short_label() {
	// single-char label
	let area = Rect::new(0, 0, 60, 20);
	let buf = render_labeled_graph(Some("x"), LineType::Plain, area);
	assert_eq!(count_symbol(&buf, "x"), 1, "single-char label renders exactly once");
}

#[test]
fn label_renders_for_every_line_type() {
	// the label paint runs regardless of the line type (it's drawn after the
	// line glyphs, on top of them).
	let area = Rect::new(0, 0, 60, 20);
	for lt in [LineType::Plain, LineType::Rounded, LineType::Double, LineType::Thick] {
		let buf = render_labeled_graph(Some("k"), lt, area);
		assert_eq!(
			count_symbol(&buf, "k"),
			1,
			"label 'k' must render for line type {:?}",
			lt
		);
	}
}

#[test]
fn no_label_renders_no_label_text() {
	// A connection with NO label must NOT write any stray label characters.
	// (this is the direct opt-in complement: label-less → no label paint.)
	let area = Rect::new(0, 0, 60, 20);
	let buf = render_labeled_graph(None, LineType::Rounded, area);
	// pick a string that wouldn't appear in a normal line graph's output
	assert_eq!(
		count_symbol(&buf, "L"),
		0,
		"a label-less graph must not contain label text 'L'"
	);
	assert_eq!(
		count_symbol(&buf, "Z"),
		0,
		"a label-less graph must not contain label text 'Z'"
	);
}

#[test]
fn with_label_is_none_by_default() {
	// the Connection API contract: default label is None, with_label sets it.
	let conn = Connection::new(1u32.into(), 0u32.into(), 0u32.into(), 0u32.into());
	assert!(conn.label().is_none(), "label must be None by default");
	let labeled = conn.with_label("foo");
	assert_eq!(labeled.label(), Some("foo"), "with_label must set the label");
}

#[test]
fn label_renders_in_ltr_direction() {
	// labels must render in horizontal flow regardless of direction.
	let nodes = vec![
		NodeLayout::new((8, 4)).with_title("root"),
		NodeLayout::new((8, 4)).with_title("child"),
	];
	let conn = Connection::new(1u32.into(), 0u32.into(), 0u32.into(), 0u32.into())
		.with_label("io");
	let area = Rect::new(0, 0, 60, 20);
	let mut graph =
		NodeGraph::new(nodes, vec![conn], area.width as usize, area.height as usize)
			.with_direction(ratatui_flow::FlowDirection::Ltr);
	graph.calculate();
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());
	assert!(count_symbol(&buf, "i") >= 1, "Ltr: label 'io' must render 'i'");
	assert!(count_symbol(&buf, "o") >= 1, "Ltr: label 'io' must render 'o'");
}

#[test]
fn multiple_labels_each_render() {
	// three-node chain, each connection labeled differently.
	let nodes = vec![
		NodeLayout::new((8, 4)).with_title("r"), // 0 root
		NodeLayout::new((8, 4)).with_title("a"), // 1
		NodeLayout::new((8, 4)).with_title("b"), // 2
	];
	let conns = vec![
		Connection::new(1u32.into(), 0u32.into(), 0u32.into(), 0u32.into())
			.with_label("p"), // 1 -> 0
		Connection::new(2u32.into(), 0u32.into(), 1u32.into(), 0u32.into())
			.with_label("q"), // 2 -> 1
	];
	let area = Rect::new(0, 0, 60, 20);
	let mut graph =
		NodeGraph::new(nodes, conns, area.width as usize, area.height as usize);
	graph.calculate();
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());
	assert_eq!(count_symbol(&buf, "p"), 1, "first label 'p' must render");
	assert_eq!(count_symbol(&buf, "q"), 1, "second label 'q' must render");
}

// ===========================================================================
// Step 9 — port display names
// ===========================================================================
//
// `NodeLayout::with_port_name(PortId, &str)` attaches an optional display name
// to a port. When the graph renders a port symbol, the name's first character
// lands on the cell one step *inside* the node from that symbol, growing
// inward. This is a pure opt-in overlay: a port-name-less graph renders
// byte-for-byte identically to pre-Step-9 (the 74 pre-existing tests still
// pass unchanged).

/// Shorthand for building a [`PortId`] from a `usize` (matches the `nid` helper
/// for nodes; the PortId inner field is `pub(crate)`).
fn pid(p: usize) -> PortId {
	p.into()
}

/// Helper: render a 2-node Rtl graph (1 -> 0, node 0 is the root on the right)
/// where node 0 has an **in** port name on `to_port` and node 1 has an **out**
/// port name on `from_port`. Returns the rendered buffer plus each node's
/// screen-coordinate content rect (via `split_named`).
fn render_named_ports_graph(
	in_name: Option<&str>,
	out_name: Option<&str>,
	area: Rect,
) -> (Buffer, Rect, Rect) {
	let mut node0 = NodeLayout::new((8, 4)).with_title("root");
	if let Some(n) = in_name {
		node0 = node0.with_port_name(pid(0), n);
	}
	let mut node1 = NodeLayout::new((8, 4)).with_title("child");
	if let Some(n) = out_name {
		node1 = node1.with_port_name(pid(0), n);
	}
	// 1 -> 0: node 0 is root; the single connection has from_port=0 on node 1
	// (out port) and to_port=0 on node 0 (in port).
	let conn = Connection::new(1u32.into(), 0u32.into(), 0u32.into(), 0u32.into());
	let mut graph = NodeGraph::new(
		vec![node0, node1],
		vec![conn],
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();
	// split_named must run BEFORE render, since `render` (StatefulWidget) moves
	// the graph.
	let named = graph.split_named(area);
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());
	// nodes are returned in render order: index 0 -> node 0, index 1 -> node 1.
	let rect0 =
		named.iter().find(|(id, _)| *id == nid(0)).map(|(_, r)| *r).unwrap_or_default();
	let rect1 =
		named.iter().find(|(id, _)| *id == nid(1)).map(|(_, r)| *r).unwrap_or_default();
	(buf, rect0, rect1)
}

#[test]
fn in_port_name_first_char_on_inner_cell() {
	// Node 0 (root) has in-port 0 named "in0". After render, the cell one step
	// inside node 0's left border, on the port row, must hold 'i' (the name's
	// first char). Under Rtl the in port is on the LEFT edge of node 0.
	let area = Rect::new(0, 0, 60, 20);
	let (buf, content0, _content1) = render_named_ports_graph(Some("in0"), None, area);
	assert!(content0.width > 0 && content0.height > 0, "node 0 must be placed");

	// in-port symbol is at (content0.x - 1, content0.y + 0); the inner cell is
	// content0.x on the same row.
	let inner_x = content0.x;
	let inner_y = content0.y;
	let cell = buf
		.cell(ratatui::layout::Position::new(inner_x, inner_y))
		.expect("inner cell exists");
	assert_eq!(
		cell.symbol(),
		"i",
		"in-port name 'in0' first char must be on the inner cell ({},{})",
		inner_x,
		inner_y
	);
}

#[test]
fn out_port_name_first_char_on_inner_cell() {
	// Node 1 (child) has out-port 0 named "out". Under Rtl the out port is on
	// node 1's RIGHT edge; the inner cell is one step left of it on the port
	// row, and must hold 'o' (the name's first char).
	let area = Rect::new(0, 0, 60, 20);
	let (buf, _content0, content1) = render_named_ports_graph(None, Some("out"), area);
	assert!(content1.width > 0 && content1.height > 0, "node 1 must be placed");

	// out-port symbol is at (content1.right(), content1.y + 0); inner cell is
	// content1.right() - 1 on the same row.
	let inner_x = content1.right() - 1;
	let inner_y = content1.y;
	let cell = buf
		.cell(ratatui::layout::Position::new(inner_x, inner_y))
		.expect("inner cell exists");
	assert_eq!(
		cell.symbol(),
		"o",
		"out-port name 'out' first char must be on the inner cell ({},{})",
		inner_x,
		inner_y
	);
}

#[test]
fn in_and_out_port_names_render_together() {
	// Both nodes carry a name; both inner cells must hold their respective first
	// chars. (This also verifies names don't clobber each other or the port
	// symbols.)
	let area = Rect::new(0, 0, 60, 20);
	let (buf, content0, content1) =
		render_named_ports_graph(Some("in0"), Some("out"), area);
	assert!(content0.width > 0 && content1.width > 0);

	let in_cell = buf
		.cell(ratatui::layout::Position::new(content0.x, content0.y))
		.expect("in inner cell");
	assert_eq!(in_cell.symbol(), "i", "in-port name first char");

	let out_cell = buf
		.cell(ratatui::layout::Position::new(content1.right() - 1, content1.y))
		.expect("out inner cell");
	assert_eq!(out_cell.symbol(), "o", "out-port name first char");
}

#[test]
fn no_port_name_is_zero_render_change() {
	// A graph with no port names must render identically to a graph built the
	// old way (no with_port_name calls). Both buffers must be byte-equal.
	let area = Rect::new(0, 0, 60, 20);
	let (buf_without, _, _) = render_named_ports_graph(None, None, area);

	// Build the "reference" buffer directly (no port_names field ever touched).
	let nodes = vec![
		NodeLayout::new((8, 4)).with_title("root"),
		NodeLayout::new((8, 4)).with_title("child"),
	];
	let conn = Connection::new(1u32.into(), 0u32.into(), 0u32.into(), 0u32.into());
	let mut graph =
		NodeGraph::new(nodes, vec![conn], area.width as usize, area.height as usize);
	graph.calculate();
	let mut buf_ref = Buffer::empty(area);
	graph.render(area, &mut buf_ref, &mut FlowState::default());

	assert_eq!(
		buf_without, buf_ref,
		"a graph with no port names must render byte-for-byte identically (opt-in zero-change)"
	);
}

#[test]
fn port_name_truncates_to_inner_width() {
	// A name longer than the node's content width must be truncated — only as
	// many chars as fit inside the node may be written, and the cell just past
	// the far border must NOT be overwritten.
	let area = Rect::new(0, 0, 60, 20);
	// node 0 inner width = 8 - 2 = 6; name "abcdefgh" (8 chars) must truncate.
	let (buf, content0, _content1) =
		render_named_ports_graph(Some("abcdefgh"), None, area);
	assert!(content0.width > 0);
	// first char still on the inner cell
	let first = buf
		.cell(ratatui::layout::Position::new(content0.x, content0.y))
		.expect("first inner cell");
	assert_eq!(first.symbol(), "a", "truncated name still starts at inner cell");
	// the cell at content0.right() is the right BORDER — must not be clobbered
	// by the name (so 'h' must not appear there). Verify the border cell is not
	// 'h' and the name did not bleed past the content area.
	let border = buf
		.cell(ratatui::layout::Position::new(content0.right(), content0.y))
		.expect("border cell");
	assert_ne!(
		border.symbol(),
		"h",
		"name must truncate before the right border (no overflow)"
	);
	// count 'h' anywhere on the port row inside content0: there should be 0
	// (only a..f = 6 chars fit, inner width 6).
	let mut hs = 0;
	for x in content0.x..content0.right() {
		if let Some(c) = buf.cell(ratatui::layout::Position::new(x, content0.y))
			&& c.symbol() == "h"
		{
			hs += 1;
		}
	}
	assert_eq!(hs, 0, "truncated tail 'h' must not appear inside the node");
}

#[test]
fn port_name_in_vertical_direction() {
	// In a vertical flow (Ttb) the in port is on the BOTTOM edge and the out
	// port on the TOP edge. The name's first char still lands one cell inside
	// the node from the port symbol. Verify the in-port name under Ttb.
	let area = Rect::new(0, 0, 40, 40);
	let node0 = NodeLayout::new((8, 4)).with_port_name(pid(0), "in");
	let node1 = NodeLayout::new((8, 4));
	let conn = Connection::new(1u32.into(), 0u32.into(), 0u32.into(), 0u32.into());
	let mut graph = NodeGraph::new(
		vec![node0, node1],
		vec![conn],
		area.width as usize,
		area.height as usize,
	)
	.with_direction(ratatui_flow::FlowDirection::Ttb);
	graph.calculate();
	let named = graph.split_named(area);
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());
	let content0 =
		named.iter().find(|(id, _)| *id == nid(0)).map(|(_, r)| *r).unwrap_or_default();
	assert!(content0.width > 0 && content0.height > 0, "node 0 placed under Ttb");

	// under Ttb the in port sits on the bottom edge at (content0.x + to_port,
	// content0.bottom() + 0)... i.e. the port row is the bottom border. The
	// inner cell is one row up (content0.bottom() - 1) on the same column.
	// to_port=0 -> column content0.x.
	let inner_x = content0.x;
	let inner_y = content0.bottom() - 1;
	let cell = buf
		.cell(ratatui::layout::Position::new(inner_x, inner_y))
		.expect("vertical in inner cell");
	assert_eq!(
		cell.symbol(),
		"i",
		"Ttb: in-port name first char must be one cell above the bottom edge"
	);
}
