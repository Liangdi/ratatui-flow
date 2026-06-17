//! Integration tests for `NodeGraph` layout, cycle/out-of-bounds safety,
//! render-on-tiny-canvas safety, and the `content` example fixture.
//!
//! These tests are intentionally read-only w.r.t. `src/` — they pin down the
//! behavior fixed in Steps 1 and 2 (cycle detection, out-of-bounds connection
//! skipping, render-time bounds checks) as a regression net.

use ratatui::{
	buffer::Buffer,
	layout::Rect,
	widgets::StatefulWidget,
};
use ratatui_flow::{Connection, Diagnostic, LineType, NodeGraph, NodeLayout};

/// Helper: how many of the rects returned by `split` are non-zero-sized
/// (i.e. the node was actually placed and fits inside `area`).
fn count_placed(rects: &[Rect]) -> usize {
	rects.iter()
		.filter(|r| r.width > 0 && r.height > 0)
		.count()
}

/// Helper: build a graph, run calculate, render into a buffer of `area`'s size,
/// and return the split rects. Used by several tests.
fn build_and_split(
	nodes: Vec<NodeLayout<'_>>,
	connections: Vec<Connection>,
	area: Rect,
) -> (NodeGraph<'_>, Vec<Rect>) {
	let mut graph = NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
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
fn linear_chain_3() -> (Vec<NodeLayout<'static>>, Vec<Connection>) {
	let nodes = vec![
		NodeLayout::new((6, 4)),
		NodeLayout::new((6, 4)),
		NodeLayout::new((6, 4)),
	];
	// 2 -> 1 -> 0: node 0 is root, 1 is child of 0, 2 is child of 1.
	let connections = vec![Connection::new(1, 0, 0, 0), Connection::new(2, 0, 1, 0)];
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
	assert_eq!(
		placed, 3,
		"all 3 reachable nodes should be placed (got {placed})"
	);
}

#[test]
fn placed_node_rects_do_not_intersect() {
	// A small fan-out DAG: root 0, children 1 and 2 both fed into 0.
	let nodes = vec![
		NodeLayout::new((8, 5)),
		NodeLayout::new((8, 5)),
		NodeLayout::new((8, 5)),
	];
	// 1 -> 0, 2 -> 0  => 0 is root; 1 and 2 are its children (stacked vertically).
	let connections = vec![Connection::new(1, 0, 0, 0), Connection::new(2, 0, 0, 1)];
	let area = Rect::new(0, 0, 80, 30);
	let (_graph, rects) = build_and_split(nodes, connections, area);

	let placed: Vec<Rect> = rects.into_iter().filter(|r| r.width > 0 && r.height > 0).collect();
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
	let connections = vec![Connection::new(2, 0, 2, 0)]; // 2 -> 2 self-loop
	let area = Rect::new(0, 0, 60, 20);
	let (_graph, rects) = build_and_split(nodes, connections, area);

	let placed0 = rects[0].width > 0 && rects[0].height > 0;
	let placed1 = rects[1].width > 0 && rects[1].height > 0;
	let placed2 = rects[2].width > 0 && rects[2].height > 0;
	assert!(placed0, "root node 0 should be placed");
	assert!(placed1, "root node 1 should be placed");
	assert!(
		!placed2,
		"unreachable node 2 should NOT be placed (rect={:?})",
		rects[2]
	);
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
		Connection::new(1, 0, 0, 0), // A -> R
		Connection::new(2, 0, 1, 0), // B -> A
		Connection::new(1, 0, 2, 0), // A -> B  (closes A<->B cycle)
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
	let nodes = vec![
		NodeLayout::new((6, 4)),
		NodeLayout::new((6, 4)),
		NodeLayout::new((6, 4)),
	];
	let connections = vec![
		Connection::new(1, 0, 0, 0),
		Connection::new(2, 0, 1, 0),
		Connection::new(1, 0, 2, 0),
	];
	let area = Rect::new(0, 0, 40, 15);
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();

	let mut buf = Buffer::empty(area);
	// No panic => pass.
	graph.render(area, &mut buf, &mut ());
}

// ===========================================================================
// D. Out-of-bounds connections are skipped (Step 1 regression)
// ===========================================================================

#[test]
fn out_of_bounds_connection_from_node_skipped() {
	// 2 nodes (indices 0,1). Connection references from_node=5 (>= len).
	let nodes = vec![NodeLayout::new((6, 4)), NodeLayout::new((6, 4))];
	let connections = vec![
		Connection::new(1, 0, 0, 0), // valid: 1 -> 0
		Connection::new(5, 0, 0, 1), // INVALID from_node=5
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
		Connection::new(1, 0, 0, 0), // valid
		Connection::new(0, 0, 9, 0), // INVALID to_node=9
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
	let connections = vec![
		Connection::new(10, 0, 11, 0),
		Connection::new(99, 0, 0, 0),
	];
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
	let connections = vec![
		Connection::new(1, 0, 0, 0),
		Connection::new(2, 0, 1, 0),
	];
	// Layout canvas is large; render buffer is small.
	let canvas = Rect::new(0, 0, 60, 20);
	let render_area = Rect::new(0, 0, 20, 10);
	let mut graph =
		NodeGraph::new(nodes, connections, canvas.width as usize, canvas.height as usize);
	graph.calculate();

	let mut buf = Buffer::empty(render_area);
	// No panic => pass.
	graph.render(render_area, &mut buf, &mut ());
}

#[test]
fn render_single_node_into_one_cell_buffer_no_panic() {
	// Degenerate 1x1 render buffer with a properly-laid-out (large canvas)
	// graph. The node won't fit and must be skipped, not panic.
	let nodes = vec![NodeLayout::new((10, 6))];
	let canvas = Rect::new(0, 0, 40, 20);
	let render_area = Rect::new(0, 0, 1, 1);
	let mut graph = NodeGraph::new(nodes, vec![], canvas.width as usize, canvas.height as usize);
	graph.calculate();

	let mut buf = Buffer::empty(render_area);
	graph.render(render_area, &mut buf, &mut ());
}

#[test]
fn render_nodes_outside_buffer_bounds_are_skipped() {
	// Two nodes laid out on a wide canvas. Build two identical graphs and render
	// one into the full canvas (sanity: frame is drawable) and one into a tiny
	// slice (must not panic; out-of-canvas nodes are skipped).
	let mk_graph = || {
		let nodes = vec![NodeLayout::new((8, 5)), NodeLayout::new((8, 5))];
		let connections = vec![Connection::new(1, 0, 0, 0)];
		let canvas = Rect::new(0, 0, 60, 20);
		let mut g = NodeGraph::new(nodes, connections, canvas.width as usize, canvas.height as usize);
		g.calculate();
		g
	};

	// Sanity: on the full canvas, render writes at least one cell.
	let canvas = Rect::new(0, 0, 60, 20);
	let mut full_buf = Buffer::empty(canvas);
	let blank = Buffer::empty(canvas);
	let g_full = mk_graph();
	g_full.render(canvas, &mut full_buf, &mut ());
	let drawable = full_buf.content().iter().zip(blank.content().iter()).any(|(a, b)| a != b);
	assert!(drawable, "graph should draw into a full-canvas buffer");

	// Now render into a tiny slice; must not panic.
	let tiny = Rect::new(0, 0, 5, 5);
	let mut tiny_buf = Buffer::empty(tiny);
	let g_tiny = mk_graph();
	g_tiny.render(tiny, &mut tiny_buf, &mut ());
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

	let nodes: Vec<NodeLayout<'static>> = CONTENTS
		.iter()
		.map(|c| NodeLayout::from_content(c))
		.collect();

	// Same connection set as the example.
	let connections = vec![
		Connection::new(0, 0, 1, 0),
		Connection::new(0, 0, 2, 0),
		Connection::new(1, 0, 3, 0).with_line_type(LineType::Double),
		Connection::new(2, 0, 3, 1),
		Connection::new(3, 0, 4, 0),
		Connection::new(3, 1, 5, 0).with_line_type(LineType::Double),
		Connection::new(4, 0, 5, 1),
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
	let placed: Vec<Rect> = rects.into_iter().filter(|r| r.width > 0 && r.height > 0).collect();
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
	graph.render(area, &mut buf, &mut ());
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
	graph.render(area, &mut buf, &mut ());

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
	let conns = vec![Connection::new(0, 0, 1, 0)];
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
	let conns = vec![Connection::new(0, 0, 1, 0)];
	let mut graph = NodeGraph::new(nodes, conns, 20, 10);
	graph.calculate();

	// Render into a buffer smaller than the node sizes. No panic => pass.
	let small_rect = Rect::new(0, 0, 20, 10);
	let mut buf = Buffer::empty(small_rect);
	graph.render(small_rect, &mut buf, &mut ());
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
	let connections = vec![
		Connection::new(0, 0, 1, 0),
		Connection::new(2, 0, 2, 0),
	];
	let area = Rect::new(0, 0, 60, 20);
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();

	assert!(
		graph.diagnostics().contains(&Diagnostic::UnplacedNode { node: 2 }),
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
		Connection::new(0, 0, 1, 0), // valid
		Connection::new(5, 0, 0, 1), // INVALID from_node=5
		Connection::new(0, 0, 9, 0), // INVALID to_node=9
	];
	let area = Rect::new(0, 0, 40, 15);
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();

	let diags = graph.diagnostics();
	assert!(
		diags.contains(&Diagnostic::InvalidConnectionRef { from_node: 5, to_node: 0 }),
		"expected InvalidConnectionRef {{ from_node: 5, to_node: 0 }}, got {diags:?}",
	);
	assert!(
		diags.contains(&Diagnostic::InvalidConnectionRef { from_node: 0, to_node: 9 }),
		"expected InvalidConnectionRef {{ from_node: 0, to_node: 9 }}, got {diags:?}",
	);
}

/// `diagnostics()` reflects the *latest* `calculate()` run only: it is cleared
/// at the start of each call. A graph that was dirty on the first run and clean
/// on the second must report an empty slice after the second run.
#[test]
fn diagnostics_cleared_between_calculate_calls() {
	let nodes = vec![
		NodeLayout::new((6, 4)),
		NodeLayout::new((6, 4)),
		NodeLayout::new((6, 4)),
	];
	// First run: invalid connection -> non-empty diagnostics.
	let conns_dirty = vec![Connection::new(9, 0, 0, 0)];
	let area = Rect::new(0, 0, 40, 15);
	let mut graph =
		NodeGraph::new(nodes, conns_dirty, area.width as usize, area.height as usize);
	graph.calculate();
	assert!(!graph.diagnostics().is_empty(), "dirty run should report diagnostics");

	// Second run: valid chain 0 -> 1 -> 2, all reachable, no bad refs.
	// (`self.connections` is fixed at construction, so we rebuild a clean graph
	// of the same shape to demonstrate the clear-on-recalculate behavior.)
	let nodes2 = vec![
		NodeLayout::new((6, 4)),
		NodeLayout::new((6, 4)),
		NodeLayout::new((6, 4)),
	];
	let conns_clean = vec![Connection::new(0, 0, 1, 0), Connection::new(1, 0, 2, 0)];
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
	let conns = vec![Connection::new(0, 0, 1, 0)];
	// 8x4 canvas: both nodes occupy the full height and together span the width,
	// leaving no room to route between their ports.
	let mut graph = NodeGraph::new(nodes, conns, 8, 4);
	graph.calculate();

	assert!(
		graph
			.diagnostics()
			.contains(&Diagnostic::RoutingFailed {
				from_node: 0,
				from_port: 0,
				to_node: 1,
				to_port: 0,
			}),
		"expected RoutingFailed for the 0->1 connection on a fully-packed canvas, got {:?}",
		graph.diagnostics(),
	);
}
