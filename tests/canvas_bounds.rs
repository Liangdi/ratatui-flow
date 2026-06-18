//! Robustness tests: `calculate()` / `to_ascii()` must never panic when the
//! graph doesn't fit its canvas — unplaced nodes (cycles / unreachable), and
//! nodes larger than the canvas. The library degrades gracefully and reports
//! non-fatal problems via [`Diagnostic`] instead.
//!
//! These lock in a real OOB panic that once slipped through: an unplaced node
//! was rendered via `buf.set_string(0, node_id, ...)` with no bounds check, so
//! a node id >= canvas height (e.g. two nodes caught in a cycle, neither a
//! root, on a 1-row canvas) indexed the canvas buffer out of bounds.

use ratatui_flow::{Connection, Diagnostic, NodeGraph, NodeLayout};

fn c(from: usize, fp: usize, to: usize, tp: usize) -> Connection<'static> {
	Connection::new(from.into(), fp.into(), to.into(), tp.into())
}

/// Two nodes in a cycle (`0 -> 1 -> 0`) are both unreachable from any root, so
/// neither is placed. The unplaced-node fallback used to write each node's id
/// at row `node_id` with no bounds check — on a canvas shorter than the highest
/// id that indexed out of bounds and panicked. Now it must degrade gracefully.
#[test]
fn cyclic_unplaced_nodes_do_not_panic_on_tiny_canvas() {
	for &(cw, ch) in &[(1u16, 1u16), (2, 2), (3, 1), (1, 3), (5, 5)] {
		let nodes = vec![NodeLayout::from_content("A"), NodeLayout::from_content("B")];
		// 0 -> 1 and 1 -> 0: a pure cycle, no root.
		let conns = vec![c(0, 0, 1, 0), c(1, 0, 0, 0)];
		let mut graph = NodeGraph::new(nodes, conns, cw as usize, ch as usize);
		graph.calculate(); // must not panic
		// Graceful, not silent: both nodes are reported unreachable.
		let unplaced = graph
			.diagnostics()
			.iter()
			.filter(|d| matches!(d, Diagnostic::UnplacedNode { .. }))
			.count();
		assert_eq!(unplaced, 2, "both cyclic nodes reported unplaced (canvas {cw}x{ch})");
		// Exporting the canvas must also be safe.
		let _ = graph.to_ascii();
	}
}

/// Nodes larger than the canvas (in either axis) must not panic during layout,
/// routing, or canvas render. No specific output is asserted — completing the
/// call without panicking is the contract being locked in.
#[test]
fn oversize_nodes_do_not_panic() {
	for &(cw, ch) in &[(1u16, 1u16), (2, 2), (3, 3), (4, 4), (2, 5), (5, 2)] {
		for &(nw, nh) in &[(6u16, 3u16), (3, 6), (10, 10), (cw + 1, ch + 1)] {
			// A DAG (no cycle) so nodes ARE placed, but extend past the canvas.
			let nodes = vec![NodeLayout::new((nw, nh)), NodeLayout::new((4, 3))];
			let conns = vec![c(0, 0, 1, 0)];
			let mut graph = NodeGraph::new(nodes, conns, cw as usize, ch as usize);
			graph.calculate(); // must not panic
			let _ = graph.to_ascii();
		}
	}
}
