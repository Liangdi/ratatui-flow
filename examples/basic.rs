// boilerplate from from tui-rs examples

use ratatui::{
	Frame, Terminal,
	backend::CrosstermBackend,
	widgets::{BorderType, Paragraph},
};

use ratatui_flow::*;

struct App {}

impl App {
	fn new() -> Self {
		Self {}
	}
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

fn ui(f: &mut Frame, _app: &App) {
	let space = f.area();
	let mut graph = NodeGraph::new(
		vec![
			NodeLayout::new((40, 10))
				.with_title("a|b|c")
				.with_border_type(BorderType::Thick),
			NodeLayout::new((40, 10))
				.with_title("b|c")
				.with_border_type(BorderType::Thick),
			NodeLayout::new((40, 10))
				.with_title("c")
				.with_border_type(BorderType::Rounded),
			NodeLayout::new((40, 10))
				.with_title("d>c")
				.with_border_type(BorderType::Thick),
			NodeLayout::new((40, 10))
				.with_title("e|d")
				.with_border_type(BorderType::Double),
			NodeLayout::new((30, 5))
				.with_title("f>(b,e)")
				.with_border_type(BorderType::Thick),
			NodeLayout::new((30, 5))
				.with_title("g|(a,f)")
				.with_border_type(BorderType::Double),
		],
		vec![
			Connection::new(0usize.into(), 0usize.into(), 1usize.into(), 0usize.into())
				.with_line_type(LineType::Double), // a | b
			Connection::new(1usize.into(), 0usize.into(), 2usize.into(), 0usize.into())
				.with_line_type(LineType::Thick), // b | c
			Connection::new(3usize.into(), 0usize.into(), 2usize.into(), 1usize.into())
				.with_line_type(LineType::Double), // d > c
			Connection::new(4usize.into(), 0usize.into(), 3usize.into(), 0usize.into()), // e | d
			Connection::new(4usize.into(), 0usize.into(), 0usize.into(), 1usize.into()), // e | d
			Connection::new(5usize.into(), 0usize.into(), 1usize.into(), 1usize.into()), // f > b
			Connection::new(5usize.into(), 0usize.into(), 4usize.into(), 6usize.into()), // f > e
			Connection::new(6usize.into(), 0usize.into(), 0usize.into(), 0usize.into())
				.with_line_type(LineType::Double), // g | a
			Connection::new(6usize.into(), 0usize.into(), 5usize.into(), 0usize.into())
				.with_line_type(LineType::Double), // g | f
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
