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
	Frame, Terminal,
};

use tui_node_graph::*;

#[derive(Debug, Clone, Default)]
pub struct ExampleNodeGraph {
	nodes: Vec<String>,
	connections: Vec<Connection>,
}

impl NodeGraphTrait for ExampleNodeGraph {
	fn node_count(&self) -> usize { self.nodes.len() }
	fn connections_from_node(&self, idx: usize) -> Vec<Connection> {
		self.connections.iter().filter(|ea| ea.from_node == idx).map(|ea| *ea).collect()
	}
	fn connections_to_node(&self, idx: usize) -> Vec<Connection> {
		self.connections.iter().filter(|ea| ea.to_node == idx).map(|ea| *ea).collect()
	}

	fn node_name(&self, node: usize) -> Option<&str> {
		self.nodes.get(node).map(|inner| inner.as_str())
	}
}

struct App {
	graph: ExampleNodeGraph,
}

impl App {
	fn new() -> Self {
		Self {
			graph: ExampleNodeGraph {
				nodes: vec!["test".into(), "second".into(), "other".into()],
				connections: vec![Connection::new(0,0,2,2)]
			}
		}
	}
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
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
	mut app: App,
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

fn ui<B: Backend>(f: &mut Frame<B>, app: &App) {
    let space = f.size();
    f.render_stateful_widget(NodeGraph(&app.graph), space, &mut ());
}
