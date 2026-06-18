//! Integration tests for the `LayoutMode` API on `NodeGraph`:
//! `LayoutMode::Auto` (the default) and `LayoutMode::Manual` (explicit
//! coordinates via `set_position` / `with_position` / `clear_position`).
//! These mirror the style of `tests/export.rs` (same `c` / `nid` helpers) and
//! are read-only w.r.t. `src/`.

use ratatui_flow::{Connection, Diagnostic, LayoutMode, NodeGraph, NodeId, NodeLayout};

/// Shorthand for `Connection::new` (matches `tests/api.rs`): terse callsites.
fn c(from: usize, from_port: usize, to: usize, to_port: usize) -> Connection<'static> {
	Connection::new(from.into(), from_port.into(), to.into(), to_port.into())
}

/// Shorthand for building a [`NodeId`] from a `usize` (matches `tests/api.rs`).
fn nid(n: usize) -> NodeId {
	n.into()
}

// ===========================================================================
// defaults / read-back
// ===========================================================================

/// A freshly-built graph defaults to `LayoutMode::Auto` (the original
/// behavior). Guards the byte-for-byte default.
#[test]
fn default_layout_mode_is_auto() {
	let nodes = vec![NodeLayout::from_content("A")];
	let graph = NodeGraph::new(nodes, vec![], 20, 8);
	assert_eq!(graph.layout_mode(), LayoutMode::Auto);
}

/// `manual_positions()` reports what `set_position` recorded.
#[test]
fn manual_positions_readback() {
	let nodes = vec![NodeLayout::from_content("A"), NodeLayout::from_content("B")];
	let mut graph = NodeGraph::new(nodes, vec![], 30, 10);
	graph.set_layout_mode(LayoutMode::Manual);
	graph.set_position(nid(0), 1, 2);
	graph.set_position(nid(1), 9, 7);

	let pos = graph.manual_positions();
	assert_eq!(pos.get(&nid(0)), Some(&(1, 2)));
	assert_eq!(pos.get(&nid(1)), Some(&(9, 7)));
	assert_eq!(pos.len(), 2);
}

/// `with_position` builder records positions before `calculate`.
#[test]
fn with_position_builder_records() {
	let nodes = vec![NodeLayout::from_content("A")];
	let graph = NodeGraph::new(nodes, vec![], 30, 10)
		.with_layout_mode(LayoutMode::Manual)
		.with_position(nid(0), 3, 4);
	assert_eq!(graph.manual_positions().get(&nid(0)), Some(&(3, 4)));
	assert_eq!(graph.layout_mode(), LayoutMode::Manual);
}

// ===========================================================================
// Manual mode placement
// ===========================================================================

/// In Manual mode, every node with a recorded position is placed at exactly
/// `Rect::new(x, y, size.0, size.1)` after `calculate`.
#[test]
fn manual_places_at_set_coords() {
	// Three nodes of known size, no connections (Manual mode doesn't need them
	// to be reachable).
	let nodes = vec![
		NodeLayout::new((6, 3)),  // node 0
		NodeLayout::new((8, 4)),  // node 1
		NodeLayout::new((10, 5)), // node 2
	];
	let mut graph = NodeGraph::new(nodes, vec![], 60, 30);
	graph.set_layout_mode(LayoutMode::Manual);
	graph.set_position(nid(0), 0, 0);
	graph.set_position(nid(1), 10, 5);
	graph.set_position(nid(2), 25, 12);
	graph.calculate();

	assert_eq!(graph.node_rect(nid(0)), Some(ratatui::layout::Rect::new(0, 0, 6, 3)));
	assert_eq!(graph.node_rect(nid(1)), Some(ratatui::layout::Rect::new(10, 5, 8, 4)));
	assert_eq!(graph.node_rect(nid(2)), Some(ratatui::layout::Rect::new(25, 12, 10, 5)));
}

/// A node in Manual mode without a `set_position` becomes an
/// `UnplacedNode` diagnostic and gets no placement.
#[test]
fn manual_unset_node_becomes_diagnostic() {
	let nodes = vec![
		NodeLayout::new((6, 3)), // node 0 — placed
		NodeLayout::new((6, 3)), // node 1 — NOT placed
	];
	let mut graph = NodeGraph::new(nodes, vec![], 30, 15);
	graph.set_layout_mode(LayoutMode::Manual);
	graph.set_position(nid(0), 0, 0);
	graph.calculate();

	// node 1 has no rect.
	assert!(graph.node_rect(nid(1)).is_none(), "unset node has no rect");
	// ...and is reported as UnplacedNode.
	let has = graph
		.diagnostics()
		.iter()
		.any(|d| matches!(d, Diagnostic::UnplacedNode { node } if *node == nid(1)));
	assert!(has, "UnplacedNode diagnostic present for node 1");
	// node 0 must NOT be reported as unplaced.
	let misplaced = graph
		.diagnostics()
		.iter()
		.any(|d| matches!(d, Diagnostic::UnplacedNode { node } if *node == nid(0)));
	assert!(!misplaced, "placed node 0 not reported unplaced");
}

// ===========================================================================
// Persistence across re-calculate / clear_position
// ===========================================================================

/// Manual positions survive multiple `calculate()` calls (they are user state,
/// not cleared by calculate). This proves `manual_positions` isn't accidentally
/// reset.
#[test]
fn manual_positions_survive_recalculate() {
	let nodes = vec![NodeLayout::new((6, 3)), NodeLayout::new((6, 3))];
	let mut graph = NodeGraph::new(nodes, vec![], 30, 15);
	graph.set_layout_mode(LayoutMode::Manual);
	graph.set_position(nid(0), 2, 3);
	graph.set_position(nid(1), 12, 8);

	graph.calculate();
	let first0 = graph.node_rect(nid(0));
	let first1 = graph.node_rect(nid(1));
	graph.calculate();
	assert_eq!(graph.node_rect(nid(0)), first0, "node 0 placement stable");
	assert_eq!(graph.node_rect(nid(1)), first1, "node 1 placement stable");
	// And the manual_positions map is still populated.
	assert_eq!(graph.manual_positions().len(), 2);
}

/// `clear_position` removes a node's explicit coordinate; after re-calculate it
/// becomes an `UnplacedNode` while the other node stays placed.
#[test]
fn clear_position_drops_placement() {
	let nodes = vec![NodeLayout::new((6, 3)), NodeLayout::new((6, 3))];
	let mut graph = NodeGraph::new(nodes, vec![], 30, 15);
	graph.set_layout_mode(LayoutMode::Manual);
	graph.set_position(nid(0), 0, 0);
	graph.set_position(nid(1), 10, 5);
	graph.calculate();
	assert!(graph.node_rect(nid(1)).is_some());

	graph.clear_position(nid(1));
	assert!(!graph.manual_positions().contains_key(&nid(1)));
	graph.calculate();
	assert!(graph.node_rect(nid(1)).is_none(), "cleared node no longer placed");
	let has = graph
		.diagnostics()
		.iter()
		.any(|d| matches!(d, Diagnostic::UnplacedNode { node } if *node == nid(1)));
	assert!(has, "cleared node reported as UnplacedNode");
	// node 0 unaffected.
	assert!(graph.node_rect(nid(0)).is_some());
}

// ===========================================================================
// Manual mode + routing
// ===========================================================================

/// In Manual mode with two connected nodes both placed, routing still runs over
/// the manual placements: the connection does NOT produce a `RoutingFailed`
/// diagnostic, and the skeleton shows a connection glyph between them.
#[test]
fn manual_routing_works() {
	// Two nodes connected 1 -> 0, placed far enough apart that the router has
	// room to find a path. Canvas is generous.
	let nodes = vec![
		NodeLayout::new((6, 3)), // node 0 (parent)
		NodeLayout::new((6, 3)), // node 1 (child)
	];
	let conns = vec![c(1, 0, 0, 0)]; // 1 -> 0
	let mut graph = NodeGraph::new(nodes, conns, 40, 12);
	graph.set_layout_mode(LayoutMode::Manual);
	graph.set_position(nid(0), 0, 0);
	graph.set_position(nid(1), 20, 0);
	graph.calculate();

	// No RoutingFailed for the 1->0 connection.
	let routing_failed = graph.diagnostics().iter().any(|d| {
		matches!(
			d,
			Diagnostic::RoutingFailed {
				from_node,
				to_node,
				..
			} if *from_node == nid(1) && *to_node == nid(0)
		)
	});
	assert!(
		!routing_failed,
		"no RoutingFailed for 1->0 in manual mode (got {:?})",
		graph.diagnostics()
	);

	// The skeleton should contain a horizontal connector drawn by the router
	// between the two nodes. A horizontal line segment `─` is the most stable
	// glyph to assert on.
	let ascii = graph.to_ascii();
	assert!(ascii.contains('─'), "skeleton shows a connector between nodes");
}

// ===========================================================================
// Auto default unchanged
// ===========================================================================

/// A graph built WITHOUT `set_layout_mode` still auto-layouts: every reachable
/// node gets a non-None rect after `calculate`. Guards the default against
/// accidentally becoming Manual.
#[test]
fn auto_default_still_layouts() {
	let nodes = vec![
		NodeLayout::from_content("ROOT"),  // node 0 (root)
		NodeLayout::from_content("child"), // node 1 (child of 0)
	];
	let conns = vec![c(1, 0, 0, 0)]; // 1 -> 0
	let mut graph = NodeGraph::new(nodes, conns, 40, 12);
	// Intentionally do NOT call set_layout_mode.
	graph.calculate();

	assert!(graph.node_rect(nid(0)).is_some(), "root placed in auto mode");
	assert!(graph.node_rect(nid(1)).is_some(), "child placed in auto mode");
	// No UnplacedNode for the two reachable nodes.
	let unplaced = graph
		.diagnostics()
		.iter()
		.any(|d| matches!(d, Diagnostic::UnplacedNode { node } if *node == nid(0) || *node == nid(1)));
	assert!(!unplaced, "reachable nodes not reported unplaced in auto mode");
}

/// `set_position` works even before the node exists (pre-declaring), and the
/// entry is retained (not validated away).
#[test]
fn set_position_before_node_exists() {
	let mut graph = NodeGraph::new(vec![], vec![], 30, 15);
	graph.set_layout_mode(LayoutMode::Manual);
	// Pre-declare a position for a node that isn't in the graph yet.
	graph.set_position(nid(5), 7, 8);
	assert_eq!(graph.manual_positions().get(&nid(5)), Some(&(7, 8)));

	// Add the node and calculate: it lands at the pre-declared spot.
	graph.add_node_with_id(nid(5), NodeLayout::new((6, 3))).unwrap();
	graph.calculate();
	assert_eq!(graph.node_rect(nid(5)), Some(ratatui::layout::Rect::new(7, 8, 6, 3)));
}

// ===========================================================================
// Pinned mode (Step 3)
// ===========================================================================
//
// `LayoutMode::Pinned` blends Auto and Manual: nodes in `manual_positions`
// are immovable anchors at their fixed coords; every other node auto-layouts
// around them, treating the pinned rects as obstacles.

use ratatui::layout::Rect;

/// Two rects are disjoint when they share no cell. `Rect::intersects` is the
/// inverse (true on any overlap), so this is just `!a.intersects(b)`.
fn rects_disjoint(a: Rect, b: Rect) -> bool {
	!a.intersects(b)
}

/// A pinned anchor stays at its fixed coordinate, and the two non-pinned
/// children connected to it are auto-placed (non-None) without overlapping the
/// anchor's rect.
#[test]
fn pinned_anchor_children_placed_around_it() {
	// node 0 = pinned parent (root); nodes 1 and 2 = children feeding INTO 0.
	// Default direction is Rtl (horizontal, main axis = x). Pin node 0 away from
	// the origin so its children — which advance along the main axis past its
	// right edge — have a clearly disjoint lane.
	let nodes = vec![
		NodeLayout::new((8, 3)), // node 0 — pinned anchor
		NodeLayout::new((6, 3)), // node 1 — child of 0
		NodeLayout::new((6, 3)), // node 2 — child of 0
	];
	let conns = vec![c(1, 0, 0, 0), c(2, 0, 0, 0)]; // 1->0, 2->0
	let mut graph = NodeGraph::new(nodes, conns, 60, 20);
	graph.set_layout_mode(LayoutMode::Pinned);
	// Pin node 0 at a fixed top-left.
	graph.set_position(nid(0), 5, 2);
	graph.calculate();

	let pinned = graph.node_rect(nid(0)).expect("pinned node placed");
	assert_eq!(pinned, Rect::new(5, 2, 8, 3), "pinned node at its fixed coord");

	let r1 = graph.node_rect(nid(1)).expect("child 1 placed");
	let r2 = graph.node_rect(nid(2)).expect("child 2 placed");

	// Neither child may overlap the pinned anchor.
	assert!(rects_disjoint(r1, pinned), "child 1 {:?} overlaps anchor {:?}", r1, pinned);
	assert!(rects_disjoint(r2, pinned), "child 2 {:?} overlaps anchor {:?}", r2, pinned);
	// And the two children must not overlap each other.
	assert!(rects_disjoint(r1, r2), "children overlap: {:?} vs {:?}", r1, r2);
}

/// A pinned node is immovable: its rect is identical across recalculate and
/// unaffected by a second pinned node placed nearby.
#[test]
fn pinned_node_is_immovable() {
	let nodes = vec![
		NodeLayout::new((6, 3)), // node 0 — pinned
		NodeLayout::new((6, 3)), // node 1 — child of 0
		NodeLayout::new((6, 3)), // node 2 — a second pinned node
	];
	let conns = vec![c(1, 0, 0, 0)]; // 1 -> 0
	let mut graph = NodeGraph::new(nodes, conns, 60, 20);
	graph.set_layout_mode(LayoutMode::Pinned);
	graph.set_position(nid(0), 4, 1);
	graph.calculate();

	let first = graph.node_rect(nid(0)).expect("pinned node 0 placed");
	assert_eq!(first, Rect::new(4, 1, 6, 3));

	// Add pressure: pin a second node nearby and recalculate. Node 0 must not
	// budge from its fixed (4, 1).
	graph.set_position(nid(2), 20, 1);
	graph.calculate();
	let second = graph.node_rect(nid(0)).expect("pinned node 0 still placed");
	assert_eq!(second, Rect::new(4, 1, 6, 3), "pinned node 0 immovable across recalc");

	// And node 2 holds its fixed coord too.
	let p2 = graph.node_rect(nid(2)).expect("pinned node 2 placed");
	assert_eq!(p2, Rect::new(20, 1, 6, 3), "pinned node 2 at its fixed coord");
}

/// Baseline sanity: in Pinned mode without overlap pressure, non-pinned nodes
/// get valid, mutually-disjoint placements. (Lenient — does not pin anything
/// to Auto's exact coords.)
#[test]
fn pinned_matches_auto_when_no_pressure() {
	// A small chain 1->0 with no pins: Pinned mode degrades to Auto (nothing
	// is pre-populated), so both nodes must place and not overlap.
	let nodes = vec![
		NodeLayout::from_content("ROOT"),  // node 0
		NodeLayout::from_content("child"), // node 1
	];
	let conns = vec![c(1, 0, 0, 0)];
	let mut graph = NodeGraph::new(nodes, conns, 40, 12);
	graph.set_layout_mode(LayoutMode::Pinned);
	graph.calculate();

	let r0 = graph.node_rect(nid(0)).expect("root placed");
	let r1 = graph.node_rect(nid(1)).expect("child placed");
	assert!(r0.width > 0 && r0.height > 0, "root has non-zero rect");
	assert!(r1.width > 0 && r1.height > 0, "child has non-zero rect");
	assert!(rects_disjoint(r0, r1), "no-pressure nodes must not overlap");
	// No diagnostics for the reachable, cleanly-placed nodes.
	let unplaced = graph
		.diagnostics()
		.iter()
		.any(|d| matches!(d, Diagnostic::UnplacedNode { node } if *node == nid(0) || *node == nid(1)));
	assert!(!unplaced, "reachable nodes not reported unplaced in pinned mode");
}

/// Pinned mode never panics or hangs when a pinned rect forces an unavoidable
/// overlap. The canvas is big enough for each node individually (so we exercise
/// Pinned's own logic, not the unrelated routing bounds check), but a pinned
/// node placed where the auto node would land makes a collision-free layout
/// impossible — Pinned degrades gracefully and `calculate()` returns.
#[test]
fn pinned_never_panics_on_pathological_canvas() {
	// Two 6x3 nodes; canvas is 6 wide. Pin node 0 so the child (which advances
	// along the main axis past node 0's right edge) is pushed off-canvas: a
	// collision-free placement is impossible. The call must terminate without
	// panicking or looping; the pinned node keeps its fixed rect.
	let nodes = vec![
		NodeLayout::new((6, 3)), // node 0 — pinned
		NodeLayout::new((6, 3)), // node 1 — child, can't be placed cleanly
	];
	let conns = vec![c(1, 0, 0, 0)];
	let mut graph = NodeGraph::new(nodes, conns, 6, 6);
	graph.set_layout_mode(LayoutMode::Pinned);
	graph.set_position(nid(0), 0, 0);
	// If Pinned's loops were unbounded, this would hang/overflow — completing
	// is the assertion.
	graph.calculate();
	// The pinned anchor is always preserved at its fixed coordinate.
	assert_eq!(
		graph.node_rect(nid(0)),
		Some(Rect::new(0, 0, 6, 3)),
		"pinned rect preserved under pressure"
	);
	// node 1 may or may not end up placed cleanly, but the call returned.
}

/// Toggling from Pinned back to Auto restores pure auto-layout: the previously
/// pinned node is no longer fixed (its `manual_positions` entry is inert in
/// Auto) and placements come out identical to a graph that never pinned.
#[test]
fn pinned_to_auto_is_inert() {
	// Two equivalent graphs: one built in Pinned then switched to Auto, one
	// always Auto. After calculate in Auto mode both must produce identical
	// placements for every node (the pin entry must not leak into Auto).
	let build = |mode: LayoutMode| {
		let nodes = vec![
			NodeLayout::new((6, 3)), // node 0
			NodeLayout::new((6, 3)), // node 1, child of 0
		];
		let conns = vec![c(1, 0, 0, 0)];
		let mut g = NodeGraph::new(nodes, conns, 40, 12);
		if mode == LayoutMode::Pinned {
			g.set_layout_mode(LayoutMode::Pinned);
			g.set_position(nid(0), 12, 4);
			g.calculate();
			// Now flip back to Auto and recalc.
			g.set_layout_mode(LayoutMode::Auto);
		}
		g.calculate();
		g
	};

	let from_pinned = build(LayoutMode::Pinned);
	let pure_auto = build(LayoutMode::Auto);

	for n in 0..2 {
		assert_eq!(
			from_pinned.node_rect(nid(n)),
			pure_auto.node_rect(nid(n)),
			"node {} placement identical after Pinned->Auto toggle",
			n
		);
	}
}
