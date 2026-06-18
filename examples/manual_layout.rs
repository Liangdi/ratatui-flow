// Manual layout: place every node at an explicit (x, y) coordinate.
//
// Demonstrates `LayoutMode::Manual` + `set_position` / `with_position`. Unlike
// `Auto` (the default), Manual skips the built-in recursive layout entirely —
// you own every coordinate. We use `Ltr` so `set_position`'s x maps directly to
// the on-screen column (the default `Rtl`/`Btt` mirror the main axis).
//
// The same diamond DAG is rendered twice — first auto-laid-out, then
// hand-placed into a diamond — so the difference is visible.

use ratatui_flow::{
	Connection, FlowDirection, LayoutMode, NodeGraph, NodeId, NodeLayout,
};

/// A 4-node diamond: Start -> {Step A, Step B} -> End.
fn diamond() -> (Vec<NodeLayout<'static>>, Vec<Connection<'static>>) {
	let nodes = vec![
		NodeLayout::from_content("Start\ninput").with_title("a"),
		NodeLayout::from_content("Step A\nvalidate").with_title("b"),
		NodeLayout::from_content("Step B\nenrich").with_title("c"),
		NodeLayout::from_content("End\noutput").with_title("d"),
	];
	let conns = vec![
		Connection::new(0usize.into(), 0usize.into(), 1usize.into(), 0usize.into()),
		Connection::new(0usize.into(), 0usize.into(), 2usize.into(), 0usize.into()),
		Connection::new(1usize.into(), 0usize.into(), 3usize.into(), 0usize.into()),
		Connection::new(2usize.into(), 0usize.into(), 3usize.into(), 0usize.into()),
	];
	(nodes, conns)
}

fn main() {
	let bodies = ["Start\ninput", "Step A\nvalidate", "Step B\nenrich", "End\noutput"];
	let lookup = |id: NodeId| bodies.get(id.as_u32() as usize).copied();

	// --- Auto (default): the built-in recursive layout places everything. ---
	let (nodes, conns) = diamond();
	let mut g = NodeGraph::new(nodes, conns, 80, 20).with_direction(FlowDirection::Ltr);
	g.calculate();
	println!("--- Auto layout (built-in) ---");
	println!("{}", g.to_ascii_with(lookup));
	for d in g.diagnostics() {
		eprintln!("diagnostic: {d:?}");
	}

	// --- Manual: every node placed by an explicit (x, y). ---
	// Hand-laid diamond: Start left-center, Step A / Step B stacked at the
	// middle, End right-center. Coordinates are canvas space (Ltr ⇒ no mirror),
	// so they read straight off the screen.
	let (nodes, conns) = diamond();
	let mut g = NodeGraph::new(nodes, conns, 80, 20)
		.with_direction(FlowDirection::Ltr)
		.with_layout_mode(LayoutMode::Manual)
		.with_position(NodeId::from(0usize), 4, 8)
		.with_position(NodeId::from(1usize), 26, 2)
		.with_position(NodeId::from(2usize), 26, 14)
		.with_position(NodeId::from(3usize), 52, 8);
	g.calculate();
	println!("\n--- Manual layout (explicit set_position) ---");
	println!("{}", g.to_ascii_with(lookup));
	for d in g.diagnostics() {
		eprintln!("diagnostic: {d:?}");
	}
}
