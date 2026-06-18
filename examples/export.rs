// Text export: render a small DAG as ASCII art (no terminal / no ratatui Frame).
//
// Demonstrates `NodeGraph::to_ascii()` (skeleton: borders/ports/connections) and
// `to_ascii_with(content)` (skeleton + node bodies overlaid). The graph mirrors the
// README quick-start pipeline: Source -> Parse -> {Validate, Transform}.

use ratatui_flow::{Connection, NodeGraph, NodeId, NodeLayout};

fn main() {
	// Node bodies, indexed by NodeId (the `i`-th node has id `i` here, since we
	// build them in order without any removals).
	let bodies: Vec<&str> = vec![
		"Source\n/data/input.csv",
		"Parse\nheader row",
		"Validate\nschema check",
		"Transform\nnormalize -> [0,1]",
	];

	let nodes = vec![
		NodeLayout::from_content(bodies[0]).with_title("src"),
		NodeLayout::from_content(bodies[1]).with_title("parse"),
		NodeLayout::from_content(bodies[2]).with_title("valid"),
		NodeLayout::from_content(bodies[3]).with_title("xform"),
	];

	let conns = vec![
		Connection::new(0usize.into(), 0usize.into(), 1usize.into(), 0usize.into()), // src -> parse
		Connection::new(1usize.into(), 0usize.into(), 2usize.into(), 0usize.into()), // parse -> valid
		Connection::new(1usize.into(), 0usize.into(), 3usize.into(), 0usize.into()), // parse -> xform
	];

	let mut graph = NodeGraph::new(nodes, conns, 100, 20);
	graph.calculate();

	// `calculate()` may surface non-fatal problems (unreachable nodes, routing
	// that fell back to an alias glyph, ...). We surface them on stderr so the
	// stdout flowcharts stay clean.
	for d in graph.diagnostics() {
		eprintln!("diagnostic: {d:?}");
	}

	println!("--- skeleton (to_ascii) ---");
	println!("{}", graph.to_ascii());

	println!();
	println!("--- full graph (to_ascii_with) ---");
	let lookup = |id: NodeId| bodies.get(id.as_u32() as usize).copied();
	println!("{}", graph.to_ascii_with(lookup));
}
