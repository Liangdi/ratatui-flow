#[macro_use] extern crate log;
use simplelog as lg;

// boilerplate from from tui-rs examples

use crossterm::{
	event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
	execute,
	terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
	io,
	time::{Duration, Instant},
};
use tui::{
	backend::{Backend, CrosstermBackend},
	Frame, Terminal, widgets::Paragraph
};

use tui_node_graph::*;

struct App {}

impl App {
	fn new() -> Self { Self {} }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut log_config = lg::ConfigBuilder::new();
	lg::WriteLogger::init(
		lg::LevelFilter::Trace,
		log_config.build(),
		std::fs::File::create("basic.log").unwrap()
	).unwrap();
	info!(target: "log", "log started");
	// setup terminal
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
	let backend = CrosstermBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	// create app and run it
	let tick_rate = Duration::from_millis(250);
	let app = App::new();
	let res = run_app(&mut terminal, app, tick_rate);

	// restore terminal
	disable_raw_mode()?;
	execute!(
		terminal.backend_mut(),
		LeaveAlternateScreen,
		DisableMouseCapture
	)?;
	terminal.show_cursor()?;

	if let Err(err) = res {
		println!("{:?}", err)
	}

	Ok(())
}

fn run_app<B: Backend>(
	terminal: &mut Terminal<B>,
	app: App,
	tick_rate: Duration,
) -> io::Result<()> {
	let mut last_tick = Instant::now();
	loop {
		terminal.draw(|f| ui(f, &app))?;

		let timeout = tick_rate
			.checked_sub(last_tick.elapsed())
			.unwrap_or_else(|| Duration::from_secs(0));
		if crossterm::event::poll(timeout)? {
			if let Event::Key(key) = event::read()? {
				if let KeyCode::Char('q') = key.code {
					return Ok(());
				}
			}
		}
		if last_tick.elapsed() >= tick_rate {
			last_tick = Instant::now();
		}
	}
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
			Connection::new(5,0,1,0), // f > b
			Connection::new(5,0,4,6), // f > e
			Connection::new(6,0,0,0), // g | a
			Connection::new(6,0,5,0), // g | f
		],
	);
	graph.calculate();
	let zones = graph.split(space);
	for (idx, ea_zone) in zones.into_iter().enumerate() {
		f.render_widget(Paragraph::new(format!("{idx}")), ea_zone);
	}
	f.render_stateful_widget(graph, space, &mut ());
}
