// Demonstrates runtime mutation of a `NodeGraph`: adding and removing nodes
// and connections after construction, then re-running `calculate`.
//
// The example builds a 3-node pipeline, prints it; adds a 4th node + a
// connection, prints it again; then removes the middle node (which cascades
// its connections), prints a final time. After each mutation we call
// `calculate` and print the diagnostics + a small rendering of the canvas so
// you can see the graph change.
//
// Like `tiny`, this renders to an in-memory buffer and prints the result, so
// it works in any terminal without taking over the screen.

use ratatui::{
	Frame, Terminal, TerminalOptions, Viewport,
	backend::CrosstermBackend,
	layout::Rect,
	style::{Modifier, Style},
	widgets::{BorderType, Paragraph},
};
use ratatui_flow::*;

const CANVAS: Rect = Rect { x: 0, y: 0, width: 70, height: 14 };

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut out = Vec::new();
	let backend = CrosstermBackend::new(&mut out);
	let mut terminal = Terminal::with_options(
		backend,
		TerminalOptions { viewport: Viewport::Fixed(CANVAS) },
	)?;

	// ---- Stage 0: a 3-node pipeline  A -> B -> C --------------------------
	// `new` assigns NodeId(0..n), so the nodes are A=0, B=1, C=2. The chain is
	// expressed as "from = output, to = input": C<-B<-A means A is the root
	// (rightmost).
	let mut graph = NodeGraph::new(
		vec![
			NodeLayout::new((10, 5))
				.with_title("A")
				.with_border_type(BorderType::Rounded),
			NodeLayout::new((10, 5))
				.with_title("B")
				.with_border_type(BorderType::Rounded),
			NodeLayout::new((10, 5))
				.with_title("C")
				.with_border_type(BorderType::Rounded),
		],
		vec![
			Connection::new(1usize.into(), 0usize.into(), 0usize.into(), 0usize.into()),
			Connection::new(2usize.into(), 0usize.into(), 1usize.into(), 0usize.into()),
		],
		CANVAS.width as usize,
		CANVAS.height as usize,
	);

	println!("=== Stage 0: initial 3-node pipeline (A -> B -> C) ===");
	terminal.draw(|f| render_stage(f, &mut graph, &["A", "B", "C"]))?;
	print_graph(&graph);

	// ---- Stage 1: add a 4th node D and connect it (D -> B) ----------------
	// `add_node` returns the freshly-allocated NodeId (3, since 0..3 are taken).
	let d_id = graph.add_node(
		NodeLayout::new((10, 5)).with_title("D").with_border_type(BorderType::Rounded),
	);
	assert!(graph.is_dirty(), "add_node marks the graph dirty");
	graph.add_connection(Connection::new(
		d_id,
		0usize.into(),
		1usize.into(),
		1usize.into(),
	));

	println!("\n=== Stage 1: added node D (id={d_id}) and connection D -> B ===");
	terminal.draw(|f| render_stage(f, &mut graph, &["A", "B", "C", "D"]))?;
	print_graph(&graph);

	// ---- Stage 2: remove the middle node B (id=1) -------------------------
	// This cascades: both connections touching B (A<-B and B<-C, plus the new
	// B<-D) are dropped automatically.
	let removed = graph.remove_node(NodeId::from(1u32));
	assert!(removed);
	assert!(graph.is_dirty(), "remove_node marks the graph dirty");

	println!("\n=== Stage 2: removed node B (id=1) — connections cascaded ===");
	terminal.draw(|f| render_stage(f, &mut graph, &["A", "_", "C", "D"]))?;
	print_graph(&graph);

	// After removing B, the remaining nodes A, C, D are all roots (no
	// `from_node` points at them anymore), so each is placed independently.
	// `positions()` and `split_named` show the survivors.
	println!("\n=== Final state: split_named ===");
	graph.calculate();
	for (id, rect) in graph.split_named(CANVAS) {
		println!("  node {id}: content rect {rect:?}");
	}
	println!("  positions() (full canvas frame, border included):");
	for (id, rect) in graph.positions() {
		println!("    node {id}: {rect:?}");
	}

	drop(terminal);
	Ok(())
}

/// Re-run `calculate` and render the current graph state: node titles into
/// their content rects, then borders/connections on top. `labels` is parallel
/// to the *original* construction order (NodeId(0..n)) — entries that no
/// longer exist (removed) are skipped via `has_node`.
fn render_stage(f: &mut Frame, graph: &mut NodeGraph, labels: &[&str]) {
	graph.calculate();
	let named = graph.split_named(CANVAS);
	for (id, rect) in &named {
		if rect.width == 0 || rect.height == 0 || !graph.has_node(*id) {
			continue;
		}
		// Labels are parallel to the original NodeId(0..n) construction; the
		// NodeId's Display ("Node#3") is the fallback for any id beyond labels.
		let display = format!("{}", id);
		let label = display.strip_prefix("Node#").and_then(|n| {
			let idx: usize = n.parse().ok()?;
			labels.get(idx).copied()
		});
		f.render_widget(
			Paragraph::new(label.unwrap_or("?"))
				.alignment(ratatui::layout::Alignment::Center)
				.style(Style::default().add_modifier(Modifier::BOLD)),
			*rect,
		);
	}
	// Blit the pre-rendered canvas (borders/ports/connections) on top of the
	// node content via the stateful path (default FlowState = offset (0,0), no
	// highlight). The graph is cloned per-frame because render_stateful_widget
	// consumes it; the underlying canvas render is cached, so this is cheap.
	let mut state = FlowState::default();
	f.render_stateful_widget(graph.clone(), CANVAS, &mut state);
}

/// Print diagnostics and the off-screen canvas buffer as text (so the example
/// is self-contained and doesn't require a real terminal).
fn print_graph(graph: &NodeGraph) {
	let diags = graph.diagnostics();
	if diags.is_empty() {
		println!("  diagnostics: (none)");
	} else {
		println!("  diagnostics:");
		for d in diags {
			println!("    - {d:?}");
		}
	}
}
