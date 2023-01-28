// boilerplate from from tui-rs examples

use tui::{
	backend::{Backend, CrosstermBackend},
	Frame, Terminal, widgets::Paragraph
};

use tui_nodes::*;

struct App {}

impl App {
	fn new() -> Self { Self {} }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	print!("\x1b[2J\x1b[1;1H");
	// setup terminal
	let stdout = std::io::stdout();
	let backend = CrosstermBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	// create app and run it
	let app = App::new();
	terminal.draw(|f| ui(f, &app))?;

	Ok(())
}

fn ui<B: Backend>(f: &mut Frame<B>, _app: &App) {
	let space = f.size();
	let mut graph = NodeGraph::new(
		vec![
			NodeLayout::new((40, 10)).with_title("a|b|c"),
			NodeLayout::new((40, 10)).with_title("b|c"),
			NodeLayout::new((40, 10)).with_title("c"),
			NodeLayout::new((40, 10)).with_title("d>c"),
			NodeLayout::new((40, 10)).with_title("e|d"),
			NodeLayout::new((30, 5)).with_title("f>(b,e)"),
			NodeLayout::new((30, 5)).with_title("g|(a,f)"),
		],
		vec![
			Connection::new(0,0,1,0), // a | b
			Connection::new(1,0,2,0), // b | c
			Connection::new(3,0,2,1), // d > c
			Connection::new(4,0,3,0), // e | d
			Connection::new(5,0,1,1), // f > b
			Connection::new(5,0,4,6), // f > e
			Connection::new(6,0,0,0), // g | a
			Connection::new(6,0,5,0), // g | f
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
