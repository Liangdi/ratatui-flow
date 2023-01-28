// boilerplate from from tui-rs examples

use tui::{
	backend::{Backend, CrosstermBackend},
	Frame, Terminal, widgets::Paragraph, layout::Rect
};

use tui_nodes::*;

struct App {}

impl App {
	fn new() -> Self { Self {} }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	// setup terminal
	let mut out = Vec::new();
	let backend = CrosstermBackend::new(&mut out);
	let mut terminal = Terminal::new(backend)?;

	// create app and run it
	let app = App::new();
	terminal.draw(|f| ui(f, &app))?;

	drop(terminal);

	print!("\x1b[2J\x1b[1;1H");
	println!("{}", std::str::from_utf8(&out).unwrap());

	Ok(())
}

fn ui<B: Backend>(f: &mut Frame<B>, _app: &App) {
	let space = Rect {
		x: 0, y: 0,
		width: 18,
		height: 8,
	};
	let mut graph = NodeGraph::new(
		vec![
			NodeLayout::new((4, 4)),
			NodeLayout::new((4, 4)),
			NodeLayout::new((4, 4)),
			NodeLayout::new((4, 4)),
		],
		vec![
			Connection::new(0,0,1,0),
			Connection::new(1,0,2,0),
			Connection::new(3,0,2,1),
		],
		space.width as usize,
		space.height as usize,
	);
	graph.calculate();
	let zones = graph.split(space);
	for (idx, ea_zone) in zones.into_iter().enumerate() {
		f.render_widget(Paragraph::new(format!("{idx}")), ea_zone);
	}
	f.render_stateful_widget(graph, space, &mut ());
}
