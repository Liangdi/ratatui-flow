// A more complex graph that exercises content-driven node sizing.
//
// Every node is built with `NodeLayout::from_content`, so its size auto-fits
// the text it displays (different line counts / widths per node). Each
// connection also gets its own color / line-type so they're easy to tell
// apart. The topology is a clean fan-out / fan-in DAG with no crossing edges,
// so the auto-sized nodes are the focus rather than messy routing.
//
// Renders to an in-memory buffer and prints it (like `tiny`), so you get a
// fixed-size, colored result in any terminal.

use ratatui::{
	Frame, Terminal, TerminalOptions, Viewport,
	backend::CrosstermBackend,
	layout::Rect,
	style::{Color, Modifier, Style},
	widgets::{BorderType, Paragraph},
};
use ratatui_flow::*;

const CONTENTS: [&str; 6] = [
	"Source\n/data/input.csv\n~10M rows",
	"Parse\nheader row\ninfer schema\nutf-8 decode",
	"Validate\nreject nulls\ndrop duplicates",
	"Transform\nnormalize -> [0,1]\none-hot encode\ncast types",
	"Filter\nvalue > 0.5\nregion == \"us\"",
	"Sink\nINSERT INTO events\nON CONFLICT\nDO NOTHING",
];

const TITLES: [&str; 6] = ["src", "parse", "valid", "xform", "filter", "sink"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut out = Vec::new();
	let backend = CrosstermBackend::new(&mut out);
	// CrosstermBackend on a Vec would otherwise be clipped to the live
	// terminal's size (often 80x24); pin a fixed viewport so the wider graph
	// fits without `cell_mut` going out of bounds.
	let mut terminal = Terminal::with_options(
		backend,
		TerminalOptions {
			viewport: Viewport::Fixed(Rect::new(0, 0, 120, 24)),
		},
	)?;

	terminal.draw(ui)?;

	drop(terminal);
	print!("\x1b[2J\x1b[1;1H");
	print!("{}", std::str::from_utf8(&out)?);
	// finish with a newline so the shell prompt lands on a fresh line
	println!();
	Ok(())
}

fn ui(f: &mut Frame) {
	// wide enough that the leftmost source node isn't pushed off-canvas
	let space = Rect { x: 0, y: 0, width: 120, height: 24 };

	let nodes: Vec<NodeLayout> = TITLES
		.iter()
		.zip(CONTENTS.iter())
		.enumerate()
		.map(|(i, (title, content))| {
			let mut node = NodeLayout::from_content(content)
				.with_title(title)
				.with_border_type(BorderType::Rounded);
			// Step 9: attach short display names to some ports so they render
			// one cell inside the node from each port symbol. Port ids here
			// match the connection's from_port / to_port values below.
			match i {
				0 => {
					// src: one out port feeding parse + valid
					node = node
						.with_port_name(0usize.into(), "raw")
						.with_port_name(1usize.into(), "raw");
				}
				3 => {
					// xform: an in port from parse, another from valid; out ports
					// to filter and sink.
					node = node
						.with_port_name(0usize.into(), "tok")
						.with_port_name(1usize.into(), "ok")
						.with_port_name(2usize.into(), "out")
						.with_port_name(3usize.into(), "db");
				}
				5 => {
					// sink: two in ports
					node = node
						.with_port_name(0usize.into(), "rec")
						.with_port_name(1usize.into(), "flt");
				}
				_ => {}
			}
			node
		})
		.collect();

	// from_node = output side, to_node = input side. Roots (nodes that never
	// appear as a `from_node`, i.e. final outputs) land on the right; the
	// graph opens leftward toward the sources. One color per connection.
	// A few connections carry a `.with_label(...)` so the connection's role is
	// readable at its midpoint (Step 8 feature).
	let connections = vec![
		Connection::new(0usize.into(), 0usize.into(), 1usize.into(), 0usize.into())
			.with_line_style(Style::default().fg(Color::Green))
			.with_label("raw"), // src   -> parse
		Connection::new(0usize.into(), 0usize.into(), 2usize.into(), 0usize.into())
			.with_line_style(Style::default().fg(Color::Blue)), // src   -> valid
		Connection::new(1usize.into(), 0usize.into(), 3usize.into(), 0usize.into())
			.with_line_type(LineType::Double)
			.with_line_style(Style::default().fg(Color::Yellow))
			.with_label("tokens"), // parse -> xform
		Connection::new(2usize.into(), 0usize.into(), 3usize.into(), 1usize.into())
			.with_line_style(Style::default().fg(Color::Cyan)), // valid -> xform
		Connection::new(3usize.into(), 0usize.into(), 4usize.into(), 0usize.into())
			.with_line_style(Style::default().fg(Color::Magenta))
			.with_label("out"), // xform -> filter
		Connection::new(3usize.into(), 1usize.into(), 5usize.into(), 0usize.into())
			.with_line_type(LineType::Double)
			.with_line_style(Style::default().fg(Color::Red)), // xform -> sink
		Connection::new(4usize.into(), 0usize.into(), 5usize.into(), 1usize.into())
			.with_line_style(Style::default().fg(Color::Cyan)), // filter-> sink
	];

	let mut graph =
		NodeGraph::new(nodes, connections, space.width as usize, space.height as usize);
	graph.calculate();

	// render each node's content into its auto-sized rect, then the graph
	// (borders, ports, connection lines) on top.
	let zones = graph.split(space);
	for (idx, zone) in zones.into_iter().enumerate() {
		f.render_widget(
			Paragraph::new(CONTENTS[idx])
				.style(Style::default().add_modifier(Modifier::BOLD)),
			zone,
		);
	}
	f.render_stateful_widget(graph, space, &mut FlowState::default());
}
