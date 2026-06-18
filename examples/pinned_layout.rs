// Pinned layout: pin a subset of nodes as immovable anchors; the rest
// auto-layout around them, treating the pinned rects as obstacles.
//
// Demonstrates `LayoutMode::Pinned` + `set_position`. We pin the middle node
// ("Parse") to a fixed spot and let Source / Filter / Sink flow around it. The
// same pipeline is rendered twice — pure `Auto`, then `Pinned` — so you can see
// the pin take effect. `Ltr` keeps coordinates intuitive (no main-axis mirror).

use ratatui_flow::{
	Connection, FlowDirection, LayoutMode, NodeGraph, NodeId, NodeLayout,
};

/// A 4-node linear pipeline: Source -> Parse -> Filter -> Sink.
fn pipeline() -> (Vec<NodeLayout<'static>>, Vec<Connection<'static>>) {
	let nodes = vec![
		NodeLayout::from_content("Source\ncsv").with_title("src"),
		NodeLayout::from_content("Parse\nrows").with_title("parse"),
		NodeLayout::from_content("Filter\nwhere x>0").with_title("filter"),
		NodeLayout::from_content("Sink\ndb").with_title("sink"),
	];
	let conns = vec![
		Connection::new(0usize.into(), 0usize.into(), 1usize.into(), 0usize.into()),
		Connection::new(1usize.into(), 0usize.into(), 2usize.into(), 0usize.into()),
		Connection::new(2usize.into(), 0usize.into(), 3usize.into(), 0usize.into()),
	];
	(nodes, conns)
}

fn main() {
	let bodies = ["Source\ncsv", "Parse\nrows", "Filter\nwhere x>0", "Sink\ndb"];
	let lookup = |id: NodeId| bodies.get(id.as_u32() as usize).copied();

	// --- Auto: the built-in layout owns every position. ---
	let (nodes, conns) = pipeline();
	let mut g = NodeGraph::new(nodes, conns, 80, 20).with_direction(FlowDirection::Ltr);
	g.calculate();
	println!("--- Auto layout ---");
	println!("{}", g.to_ascii_with(lookup));
	for d in g.diagnostics() {
		eprintln!("diagnostic: {d:?}");
	}

	// --- Pinned: "Parse" (node 1) is nailed to (38, 10); the rest auto-flow
	// around it as obstacles. ---
	let (nodes, conns) = pipeline();
	let mut g = NodeGraph::new(nodes, conns, 80, 20)
		.with_direction(FlowDirection::Ltr)
		.with_layout_mode(LayoutMode::Pinned)
		.with_position(NodeId::from(1usize), 38, 10);
	g.calculate();
	println!("\n--- Pinned layout (Parse pinned at (38, 10)) ---");
	println!("{}", g.to_ascii_with(lookup));
	for d in g.diagnostics() {
		eprintln!("diagnostic: {d:?}");
	}
}
