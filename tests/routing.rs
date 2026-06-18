//! Integration tests for `NodeGraph` connection-ROUTING correctness, exercised
//! purely through the PUBLIC API. Routing internals (`ConnectionsLayout`, the
//! A* router, the edge field) are all `pub(crate)`, so these tests pin the
//! *observable* outcomes:
//!
//!   - a routable graph reports zero `RoutingFailed` diagnostics AND actually
//!     draws box-drawing line glyphs into the buffer,
//!   - routed paths never cross the *interior* of a placed node (they respect
//!     node block-zones),
//!   - routing is deterministic across `calculate()` recalculates,
//!   - an unroutable connection emits `RoutingFailed` and falls back to an
//!     alias glyph (`α β γ …`),
//!   - all four `FlowDirection`s route cleanly and draw line glyphs.
//!
//! Helpers and style mirror `tests/layout.rs` (tab indentation, the `c`/`nid`
//! shorthands, `Buffer::empty` + `StatefulWidget::render`, symbol counting).

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::widgets::StatefulWidget;
use ratatui_flow::{Diagnostic, FlowDirection, FlowState, NodeGraph, NodeLayout};

/// Shorthand for `Connection::new` with `.into()` so test callsites stay terse
/// after the NodeId/PortId migration: `c(from, from_port, to, to_port)`.
fn c(
	from: usize,
	from_port: usize,
	to: usize,
	to_port: usize,
) -> ratatui_flow::Connection<'static> {
	ratatui_flow::Connection::new(
		from.into(),
		from_port.into(),
		to.into(),
		to_port.into(),
	)
}

/// The full box-drawing line glyph set the router may paint: single-light,
/// double-line, and thick-line variants (horizontal, vertical, corners, tees,
/// and the cross). Returns `true` iff `s` is one of these glyphs.
fn is_line_glyph(s: &str) -> bool {
	matches!(
		s,
		// light single
		"─" | "│" | "┌" | "┐" | "└" | "┘" | "├" | "┤" | "┬" | "┴" | "┼"
		// double-line
		| "═" | "║" | "╔" | "╗" | "╚" | "╝" | "╠" | "╣" | "╦" | "╩" | "╬"
		// thick
		| "━" | "┃" | "┏" | "┓" | "┗" | "┛" | "┣" | "┫" | "┳" | "┻" | "╋"
	)
}

/// Count cells in `buf` whose symbol satisfies `pred`.
fn count_where(buf: &Buffer, pred: impl Fn(&str) -> bool) -> usize {
	buf.content().iter().filter(|c| pred(c.symbol())).count()
}

/// Build a graph, `calculate()`, capture its diagnostics, then render into a
/// fresh buffer of `area`'s size. Returns `(diagnostics, buffer)`. The graph
/// itself is NOT returned because `StatefulWidget::render` consumes it by move;
/// diagnostics are snapshotted before render so callers can still inspect them.
fn build_calculate_render<'a>(
	nodes: Vec<NodeLayout<'a>>,
	connections: Vec<ratatui_flow::Connection<'a>>,
	area: Rect,
) -> (Vec<Diagnostic>, Buffer) {
	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();
	let diags = graph.diagnostics().to_vec();
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());
	(diags, buf)
}

// ===========================================================================
// 1. A clean multi-node DAG routes every connection and draws line glyphs.
// ===========================================================================

/// A 6-node content-style fan-out/fan-in pipeline (same shape as
/// `examples/content.rs`), built on a comfortably large 120x24 canvas. Every
/// node is reachable and every connection routes successfully.
fn pipeline_nodes_conns()
-> (Vec<NodeLayout<'static>>, Vec<ratatui_flow::Connection<'static>>) {
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
	// 0,1 -> 2,3 fan-in; 3 -> 4,5 fan-out (mix of plain and double lines).
	let connections = vec![
		c(0, 0, 2, 0),
		c(1, 0, 2, 1),
		c(2, 0, 3, 0),
		c(3, 0, 4, 0),
		c(3, 1, 5, 0).with_line_type(ratatui_flow::LineType::Double),
	];
	(nodes, connections)
}

#[test]
fn clean_dag_routes_all_connections() {
	let (nodes, conns) = pipeline_nodes_conns();
	let area = Rect::new(0, 0, 120, 24);
	let (diags, buf) = build_calculate_render(nodes, conns, area);

	let routing_failures =
		diags.iter().filter(|d| matches!(d, Diagnostic::RoutingFailed { .. })).count();
	assert_eq!(
		routing_failures, 0,
		"clean DAG must report zero RoutingFailed diagnostics, got {diags:?}",
	);

	let line_cells = count_where(&buf, is_line_glyph);
	assert!(
		line_cells > 0,
		"clean DAG must draw at least one box-drawing line glyph (drew none: routing produced no visible path)"
	);
}

// ===========================================================================
// 2. Routed connections never cross a placed node's INTERIOR content area.
// ===========================================================================

#[test]
fn routed_connections_do_not_cross_node_interiors() {
	// A 3-node fan-out: node 0 is root, nodes 1 and 2 both feed into it. Two
	// distinct connections must route around each other and around the node
	// block-zones on a large canvas.
	let nodes = vec![
		NodeLayout::new((8, 5)), // 0: root
		NodeLayout::new((8, 5)), // 1
		NodeLayout::new((8, 5)), // 2
	];
	// 1 -> 0, 2 -> 0: node 0 is root; 1 and 2 are its children.
	let connections = vec![c(1, 0, 0, 0), c(2, 0, 0, 1)];
	let area = Rect::new(0, 0, 80, 30);

	let mut graph =
		NodeGraph::new(nodes, connections, area.width as usize, area.height as usize);
	graph.calculate();
	let diags = graph.diagnostics().to_vec();
	assert_eq!(
		diags.iter().filter(|d| matches!(d, Diagnostic::RoutingFailed { .. })).count(),
		0,
		"fan-out must route cleanly before checking interiors"
	);

	// split() returns the inner CONTENT rects in screen coords — exactly the
	// "interior" region where connection line glyphs must NOT appear (node
	// borders/ports are ON the frame, one cell outside the inner rect).
	let rects = graph.split(area);
	let mut buf = Buffer::empty(area);
	graph.render(area, &mut buf, &mut FlowState::default());

	for (i, inner) in rects.iter().enumerate() {
		// Skip unplaced nodes (0×0 rects).
		if inner.width == 0 || inner.height == 0 {
			continue;
		}
		// Strictly INSIDE the inner content rect — every cell here is part of
		// the node's content area, never a routing lane.
		for y in inner.y..inner.bottom() {
			for x in inner.x..inner.right() {
				let Some(cell) = buf.cell(Position::new(x, y)) else {
					continue;
				};
				if is_line_glyph(cell.symbol()) {
					panic!(
						"connection line glyph '{}' found INSIDE node {i}'s \
						 interior content rect at ({},{}) (rect {:?})",
						cell.symbol(),
						x,
						y,
						inner
					);
				}
			}
		}
	}
	// Reachability of this point means no interior glyph was found.
	let _ = diags;
}

// ===========================================================================
// 3. Routing is deterministic across recalculates (byte-equal buffers).
// ===========================================================================

#[test]
fn routing_is_deterministic_across_recalculates() {
	// Two connections that must route around each other: a 3-node chain
	// (2 -> 1 -> 0) plus a long-hop connection (2 -> 0) crossing the middle
	// node. This gives >=2 connections with overlapping routing lanes.
	let nodes = vec![
		NodeLayout::new((8, 5)), // 0: root
		NodeLayout::new((8, 5)), // 1
		NodeLayout::new((8, 5)), // 2
	];
	let connections = vec![
		c(1, 0, 0, 0), // 1 -> 0
		c(2, 0, 1, 0), // 2 -> 1
		c(2, 1, 0, 1), // 2 -> 0 (long hop across node 1)
	];
	let area = Rect::new(0, 0, 80, 30);

	// Render twice, into independent buffers.
	let (diags1, buf1) = build_calculate_render(nodes.clone(), connections.clone(), area);
	let (diags2, buf2) = build_calculate_render(nodes, connections, area);

	// Sanity: both routed without failure and actually drew lines.
	for (label, diags) in [("first", &diags1), ("second", &diags2)] {
		let fails = diags
			.iter()
			.filter(|d| matches!(d, Diagnostic::RoutingFailed { .. }))
			.count();
		assert_eq!(fails, 0, "{label} calculate must route all connections cleanly");
	}
	assert!(
		count_where(&buf1, is_line_glyph) > 0,
		"first render must draw line glyphs for the determinism check to be meaningful"
	);

	assert_eq!(
		buf1, buf2,
		"routing must be deterministic: two recalculates of the same graph must \
		 produce byte-equal buffers"
	);
}

// ===========================================================================
// 4. An unroutable connection emits RoutingFailed AND draws an alias glyph.
// ===========================================================================

#[test]
fn unroutable_connection_emits_routing_failed_and_alias() {
	// Same fully-packed 8x4 geometry as layout.rs'
	// `diagnostics_reports_routing_failed`: two 4x4 nodes fill the canvas so
	// there is no free edge for the router to turn through.
	let nodes = vec![NodeLayout::new((4, 4)), NodeLayout::new((4, 4))];
	let connections = vec![c(0, 0, 1, 0)];
	let area = Rect::new(0, 0, 8, 4);
	let (diags, buf) = build_calculate_render(nodes, connections, area);

	// (a) The connection surfaces as RoutingFailed.
	assert!(
		diags.contains(&Diagnostic::RoutingFailed {
			from_node: 0usize.into(),
			from_port: 0usize.into(),
			to_node: 1usize.into(),
			to_port: 0usize.into(),
		}),
		"expected RoutingFailed for the 0->1 connection on a fully-packed canvas, got {diags:?}",
	);

	// (b) The fallback alias glyph (one of the Greek letters α..ω) is drawn.
	let alias_cells =
		buf.content()
			.iter()
			.filter(|cell| {
				let s = cell.symbol();
				matches!(
					s,
					"α" | "β"
						| "γ" | "δ" | "ε" | "ζ"
						| "η" | "θ" | "ι" | "κ"
						| "λ" | "μ" | "ν" | "ξ"
						| "ο" | "π" | "ρ" | "σ"
						| "τ" | "υ" | "φ" | "χ"
						| "ψ" | "ω"
				)
			})
			.count();
	assert!(
		alias_cells > 0,
		"unroutable connection must draw an alias glyph (α..ω) as the fallback, drew none"
	);
}

// ===========================================================================
// 5. All four FlowDirections route cleanly and draw line glyphs.
// ===========================================================================

#[test]
fn multiple_directions_route_cleanly() {
	let directions =
		[FlowDirection::Ltr, FlowDirection::Rtl, FlowDirection::Ttb, FlowDirection::Btt];

	for dir in directions {
		// A simple 3-node chain (2 -> 1 -> 0). Canvas tall enough for vertical
		// flows (3 nodes of height 4 + margins).
		let nodes = vec![
			NodeLayout::new((6, 4)),
			NodeLayout::new((6, 4)),
			NodeLayout::new((6, 4)),
		];
		let connections = vec![c(1, 0, 0, 0), c(2, 0, 1, 0)];
		let area = Rect::new(0, 0, 60, 30);

		let mut graph =
			NodeGraph::new(nodes, connections, area.width as usize, area.height as usize)
				.with_direction(dir);
		graph.calculate();
		// Capture diagnostics BEFORE render (render consumes the graph).
		let diags = graph.diagnostics().to_vec();
		let mut buf = Buffer::empty(area);
		graph.render(area, &mut buf, &mut FlowState::default());

		let fails = diags
			.iter()
			.filter(|d| matches!(d, Diagnostic::RoutingFailed { .. }))
			.count();
		assert_eq!(
			fails, 0,
			"{dir:?}: chain must route cleanly (no RoutingFailed), got {diags:?}"
		);

		let line_cells = count_where(&buf, is_line_glyph);
		assert!(
			line_cells > 0,
			"{dir:?}: chain must draw at least one box-drawing line glyph (routing produced no visible path)"
		);
	}
}
