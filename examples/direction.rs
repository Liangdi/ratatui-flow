//! TUI demo: render a small 3-node chain in each of the four `FlowDirection`s
//! (`Rtl`, `Ltr`, `Ttb`, `Btt`) so you can see how the direction changes which
//! edge the root anchors to and which way children flow. Direction arrows are
//! on so the flow reads at a glance.
//!
//! Runs in the alternate screen (restores your terminal on exit, even on
//! panic). Controls: `← →` / `h l` cycle direction · `1`-`4` jump · `q`/`Esc`
//! quit.
//!
//! ```sh
//! cargo run --example direction
//! ```

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
	EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::{
	Frame, Terminal,
	backend::CrosstermBackend,
	layout::{Constraint, Layout},
	style::{Modifier, Style},
	widgets::{BorderType, Paragraph},
};
use ratatui_flow::*;

/// Restores the terminal (raw mode off, leave alternate screen) on drop, even
/// if the app panics.
struct TerminalGuard;
impl Drop for TerminalGuard {
	fn drop(&mut self) {
		let _ = disable_raw_mode();
		let _ = execute!(io::stdout(), LeaveAlternateScreen);
	}
}

/// A 3-node chain where `n2` is the root (it is never a `from_node`):
///
/// ```text
///     2   (root)
///     |
///     1
///     |
///     0
/// ```
///
/// `Connection::new(from, .., to, ..)` means "from feeds into to"; with only
/// `n0` and `n1` as `from` nodes, `n2` ends up the root. The chain is short
/// enough to fit in all four directions on a standard (≥80×24) terminal —
/// vertical layouts need ~22 rows for three 4-row nodes.
fn build_graph(direction: FlowDirection, w: u16, h: u16) -> NodeGraph<'static> {
	let nodes = vec![
		NodeLayout::new((8, 4)).with_title("n0").with_border_type(BorderType::Rounded),
		NodeLayout::new((8, 4)).with_title("n1").with_border_type(BorderType::Rounded),
		NodeLayout::new((8, 4)).with_title("n2").with_border_type(BorderType::Rounded),
	];
	let conns = vec![
		Connection::new(0u32.into(), 0u32.into(), 1u32.into(), 0u32.into()), // n0 -> n1
		Connection::new(1u32.into(), 0u32.into(), 2u32.into(), 0u32.into()), // n1 -> n2
	];
	let mut graph = NodeGraph::new(nodes, conns, w as usize, h as usize)
		.with_direction(direction)
		.show_arrows(true);
	graph.calculate();
	graph
}

const DIRECTIONS: [FlowDirection; 4] =
	[FlowDirection::Rtl, FlowDirection::Ltr, FlowDirection::Ttb, FlowDirection::Btt];
const NAMES: [&str; 4] = ["Rtl (default)", "Ltr", "Ttb (vertical)", "Btt (vertical)"];

fn ui(f: &mut Frame, current: usize) {
	let area = f.area();
	let chunks =
		Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
	let status_bar = chunks[0];
	let graph_area = chunks[1];

	// status bar (fixed, regardless of direction)
	f.render_widget(
		Paragraph::new(format!(
			" FlowDirection: {:<16} ← →/h l switch · 1-4 select · q/Esc quit ",
			NAMES[current]
		))
		.style(Style::default().add_modifier(Modifier::REVERSED)),
		status_bar,
	);

	// rebuild the graph to fit the current pane (3 nodes are cheap to lay out)
	let graph = build_graph(DIRECTIONS[current], graph_area.width, graph_area.height);

	// node content (titles)
	let bold = Style::default().add_modifier(Modifier::BOLD);
	let titles = ["n0", "n1", "n2"];
	for (i, z) in graph.split(graph_area).iter().enumerate() {
		if i < titles.len() && z.width > 0 && z.height > 0 {
			f.render_widget(Paragraph::new(titles[i]).style(bold), *z);
		}
	}

	// borders / ports / connections / arrows (default FlowState = no pan/selection)
	let mut state = FlowState::default();
	f.render_stateful_widget(graph, graph_area, &mut state);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen)?;
	let _guard = TerminalGuard;
	let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

	let mut current: usize = 0;
	loop {
		terminal.draw(|f| ui(f, current))?;

		if !event::poll(Duration::from_millis(200))? {
			continue;
		}
		if let Event::Key(k) = event::read()? {
			if k.kind != KeyEventKind::Press {
				continue;
			}
			match k.code {
				KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
				KeyCode::Right | KeyCode::Char('l') => current = (current + 1) % 4,
				KeyCode::Left | KeyCode::Char('h') => current = (current + 3) % 4,
				KeyCode::Char('1') => current = 0,
				KeyCode::Char('2') => current = 1,
				KeyCode::Char('3') => current = 2,
				KeyCode::Char('4') => current = 3,
				_ => {}
			}
		}
	}
}
