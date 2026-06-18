//! Integration tests for the additive interactive/editor API on `NodeGraph`:
//! `replace_node`, `has_connection`, `connections_between`, and
//! `hit_test_port`. These mirror the style of `tests/layout.rs` (same `c` / `nid`
//! helpers) and are read-only w.r.t. `src/`.

use ratatui::layout::Rect;
use ratatui_flow::{
	AddNodeError, Connection, Diagnostic, FlowDirection, NodeGraph, NodeId, NodeLayout,
	PortId,
};

/// Shorthand for `Connection::new` (matches `tests/layout.rs`): terse callsites.
fn c(from: usize, from_port: usize, to: usize, to_port: usize) -> Connection<'static> {
	Connection::new(from.into(), from_port.into(), to.into(), to_port.into())
}

/// Shorthand for building a [`NodeId`] from a `usize` (matches `tests/layout.rs`).
fn nid(n: usize) -> NodeId {
	n.into()
}

/// Shorthand for building a [`PortId`] from a `usize`.
fn pid(p: usize) -> PortId {
	p.into()
}

// ===========================================================================
// replace_node
// ===========================================================================

/// `replace_node` swaps a node's [`NodeLayout`] in place: the [`NodeId`] is
/// preserved, connections survive, and after `calculate` the new size is
/// reflected in `node_rect`. The connection 1 -> 0 must still route (no
/// `RoutingFailed` diagnostic).
#[test]
fn replace_node_updates_layout_keeps_connections() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![NodeLayout::new((8, 4)), NodeLayout::new((8, 4))],
		vec![c(1, 0, 0, 0)], // 1 -> 0: node 0 is root, node 1 is its child.
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();
	let old_rect = graph.node_rect(nid(0)).expect("node 0 placed before replace");
	assert_eq!((old_rect.width, old_rect.height), (8, 4));
	assert!(!graph.is_dirty(), "graph clean before replace");

	// Replace node 0 with a bigger, retitled layout. Id and connections survive.
	graph
		.replace_node(nid(0), NodeLayout::new((20, 8)).with_title("big"))
		.expect("node 0 exists");
	assert!(graph.is_dirty(), "replace_node marks the graph dirty");
	assert!(graph.has_node(nid(0)), "node 0 still present (same id)");
	assert_eq!(
		graph.connections().len(),
		1,
		"connection count unchanged by replace_node"
	);

	// Recalculate: new size shows up, and the surviving connection still routes.
	graph.calculate();
	assert!(!graph.is_dirty(), "calculate clears dirty");
	let new_rect = graph.node_rect(nid(0)).expect("node 0 placed after replace");
	assert_eq!(
		(new_rect.width, new_rect.height),
		(20, 8),
		"node_rect must reflect the replaced node's new size"
	);
	assert!(
		!graph
			.diagnostics()
			.iter()
			.any(|d| matches!(d, Diagnostic::RoutingFailed { .. })),
		"surviving connection 1->0 must still route, got {:?}",
		graph.diagnostics()
	);
}

/// `replace_node` on an unknown id returns
/// [`Err(ConflictingId)`][AddNodeError::ConflictingId] and does NOT dirty the
/// graph (mirrors `add_node_with_id`'s conflict semantics).
#[test]
fn replace_node_unknown_id_returns_conflict() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![NodeLayout::new((8, 4))],
		vec![],
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();
	assert!(!graph.is_dirty(), "clean baseline before failed replace");

	let err = graph.replace_node(nid(99), NodeLayout::new((10, 5))).unwrap_err();
	assert_eq!(err, AddNodeError::ConflictingId);
	assert!(!graph.is_dirty(), "failed replace_node must not dirty the graph");
}

// ===========================================================================
// has_connection / connections_between
// ===========================================================================

/// `has_connection(from, to)` is directed: only the actual endpoint order
/// matches. A graph with a single 1 -> 0 edge reports `has_connection(1, 0)`
/// but not `(0, 1)` nor any pair touching an absent node.
#[test]
fn has_connection_directed() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![NodeLayout::new((8, 4)), NodeLayout::new((8, 4))],
		vec![c(1, 0, 0, 0)], // 1 -> 0 only
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();

	assert!(graph.has_connection(nid(1), nid(0)), "1 -> 0 exists");
	assert!(!graph.has_connection(nid(0), nid(1)), "0 -> 1 does not exist (directed)");
	assert!(
		!graph.has_connection(nid(0), nid(2)),
		"pair touching an absent node is false"
	);
}

/// `connections_between(a, b)` is undirected: both `1 -> 0` and `0 -> 1` count
/// as links between nodes 0 and 1. A two-connection cycle yields both.
#[test]
fn connections_between_both_directions() {
	let area = Rect::new(0, 0, 60, 20);
	let mut graph = NodeGraph::new(
		vec![NodeLayout::new((8, 4)), NodeLayout::new((8, 4))],
		vec![c(1, 0, 0, 0), c(0, 0, 1, 0)], // 1 -> 0 AND 0 -> 1
		area.width as usize,
		area.height as usize,
	);
	graph.calculate();

	let between = graph.connections_between(nid(0), nid(1));
	assert_eq!(between.len(), 2, "both directions count as connections between 0 and 1");
	// Order-independent: asking with the endpoints swapped yields the same set.
	let between_swapped = graph.connections_between(nid(1), nid(0));
	assert_eq!(between_swapped.len(), 2);
}

// ===========================================================================
// hit_test_port
// ===========================================================================

/// Mirror a node's layout placement into canvas coords the same way
/// `hit_test_port` (and `hit_test`/`render_to`) do, for a given direction.
/// Hard-codes the per-direction mirror (Rtl mirrors x, Btt mirrors y; the
/// `is_horizontal`/`mirror_main_axis` helpers are `pub(crate)` and so not visible
/// to integration tests).
fn canvas_rect_for(
	layout: Rect,
	dir: FlowDirection,
	canvas_w: u16,
	canvas_h: u16,
) -> Rect {
	let mut pos = layout;
	match dir {
		FlowDirection::Rtl => pos.x = canvas_w.saturating_sub(pos.right()),
		FlowDirection::Btt => pos.y = canvas_h.saturating_sub(pos.bottom()),
		FlowDirection::Ltr | FlowDirection::Ttb => {}
	}
	pos
}

/// In a horizontal (Rtl) 2-node graph 1 -> 0, `hit_test_port` must resolve the
/// in-port cell of node 0 (the `to` port, on the LEFT edge) and the out-port
/// cell of node 1 (the `from` port, on the RIGHT edge), and return `None` for a
/// point in a node's interior.
#[test]
fn hit_test_port_finds_in_and_out_ports() {
	let area = Rect::new(0, 0, 60, 20);
	let dir = FlowDirection::Rtl;
	let mut graph = NodeGraph::new(
		vec![
			NodeLayout::new((8, 4)).with_title("root"), // 0: root (to/in port 0)
			NodeLayout::new((8, 4)).with_title("child"), // 1: child (from/out port 0)
		],
		vec![c(1, 0, 0, 0)],
		area.width as usize,
		area.height as usize,
	)
	.with_direction(dir);
	graph.calculate();

	let layout0 = graph.node_rect(nid(0)).expect("node 0 placed");
	let layout1 = graph.node_rect(nid(1)).expect("node 1 placed");
	let pos0 = canvas_rect_for(layout0, dir, area.width, area.height);
	let pos1 = canvas_rect_for(layout1, dir, area.width, area.height);

	// Horizontal: in port (to) on the LEFT edge at x = pos.left(),
	// out port (from) on the RIGHT edge at x = pos.right() - 1.
	// Port index 0 -> y = pos.top() + 0 + 1.
	let in_cell = (pos0.left(), pos0.top() + 1);
	let out_cell = (pos1.right() - 1, pos1.top() + 1);

	assert_eq!(
		graph.hit_test_port(area, in_cell.0, in_cell.1),
		Some((nid(0), pid(0), true)),
		"in-port cell of node 0 must hit as (0, in)"
	);
	assert_eq!(
		graph.hit_test_port(area, out_cell.0, out_cell.1),
		Some((nid(1), pid(0), false)),
		"out-port cell of node 1 must hit as (1, out)"
	);

	// A point in node 0's interior (not on any port) must miss.
	let interior = (pos0.left() + 2, pos0.top() + 2);
	assert_eq!(
		graph.hit_test_port(area, interior.0, interior.1),
		None,
		"interior cell must not register as a port"
	);
}

/// In a vertical (Ttb) 2-node graph 1 -> 0, the in port sits on the BOTTOM edge
/// and the out port on the TOP edge. `hit_test_port` must resolve both.
#[test]
fn hit_test_port_vertical() {
	let area = Rect::new(0, 0, 40, 40);
	let dir = FlowDirection::Ttb;
	let mut graph = NodeGraph::new(
		vec![
			NodeLayout::new((8, 4)).with_title("root"), // 0: root (in port 0)
			NodeLayout::new((8, 4)).with_title("child"), // 1: child (out port 0)
		],
		vec![c(1, 0, 0, 0)],
		area.width as usize,
		area.height as usize,
	)
	.with_direction(dir);
	graph.calculate();

	let layout0 = graph.node_rect(nid(0)).expect("node 0 placed");
	let layout1 = graph.node_rect(nid(1)).expect("node 1 placed");
	let pos0 = canvas_rect_for(layout0, dir, area.width, area.height);
	let pos1 = canvas_rect_for(layout1, dir, area.width, area.height);

	// Vertical: in port (to) on the BOTTOM edge at y = pos.bottom() - 1,
	// out port (from) on the TOP edge at y = pos.top().
	// Port index 0 -> x = pos.left() + 0 + 1.
	let in_cell = (pos0.left() + 1, pos0.bottom() - 1);
	let out_cell = (pos1.left() + 1, pos1.top());

	assert_eq!(
		graph.hit_test_port(area, in_cell.0, in_cell.1),
		Some((nid(0), pid(0), true)),
		"Ttb: in-port cell of node 0 must hit as (0, in)"
	);
	assert_eq!(
		graph.hit_test_port(area, out_cell.0, out_cell.1),
		Some((nid(1), pid(0), false)),
		"Ttb: out-port cell of node 1 must hit as (1, out)"
	);
}

/// `node_canvas_rect` returns a node's bordered rect in CANVAS coordinates — the
/// `Rtl`/`Btt` main-axis mirror of its layout placement. It's what
/// `FlowState::ensure_visible` / `center_on` consume. Verify it mirrors correctly
/// per direction and that it stays in lock-step with where the stateful render
/// actually draws the border (the blit reads the canvas, so the canvas rect must
/// match a node's on-screen border at offset (0,0) over a canvas-sized area).
#[test]
fn node_canvas_rect_mirrors_per_direction() {
	// node 0 (root) layout placement is at the origin; under Rtl its canvas x is
	// canvas_w - layout_right.
	let mut g = NodeGraph::new(
		vec![NodeLayout::new((10, 5)), NodeLayout::new((10, 5))],
		vec![c(1, 0, 0, 0)],
		60,
		20,
	);
	g.calculate();
	let l0 = g.node_rect(nid(0)).expect("node 0 placed");
	let cv0 = g.node_canvas_rect(nid(0)).expect("node 0 canvas rect");
	// Rtl mirrors x about the canvas width; y unchanged.
	assert_eq!(cv0.x, 60 - l0.right(), "Rtl: canvas x = canvas_w - layout_right");
	assert_eq!(cv0.y, l0.y, "Rtl: y unchanged");
	// Unplaced / unknown node -> None.
	assert!(g.node_canvas_rect(nid(99)).is_none());

	// Ltr (no mirror): canvas rect == layout rect.
	let mut g2 = NodeGraph::new(
		vec![NodeLayout::new((10, 5)), NodeLayout::new((10, 5))],
		vec![c(1, 0, 0, 0)],
		60,
		20,
	)
	.with_direction(FlowDirection::Ltr);
	g2.calculate();
	assert_eq!(
		g2.node_canvas_rect(nid(0)).unwrap(),
		g2.node_rect(nid(0)).unwrap(),
		"Ltr: canvas rect == layout rect (no mirror)"
	);
}
