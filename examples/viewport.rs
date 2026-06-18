//! Interactive TUI: a complex compiler-pipeline node graph that is bigger
//! than the screen, with keyboard-driven viewport scrolling.
//!
//! This uses the Step 6 stateful path: the whole graph is laid out **once**
//! into an off-screen canvas (during `NodeGraph::calculate`), and each frame we:
//!   1. ask the graph for each node's *screen-coordinate* content rect via
//!      `split_stateful` (driven by a `FlowState` whose `view_offset` is the
//!      pan position), and render our own content widget into it;
//!   2. render the graph via its `StatefulWidget` impl
//!      (`f.render_stateful_widget(graph, view, &mut flow_state)`), which blits
//!      the scrolled window of the canvas (borders / ports / connections) onto
//!      the terminal and overlays selection/hover highlight.
//!
//! Scrolling is just changing `flow_state.view_offset` and redrawing; the
//! expensive layout/routing never re-runs per frame.
//!
//! Controls: `←→↑↓` or `hjkl` to scroll · `PgUp`/`PgDn` · `Home` reset ·
//! `q`/`Esc` quit.

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
	layout::Rect,
	style::{Color, Modifier, Style},
	widgets::{BorderType, Paragraph},
};
use ratatui_flow::*;

/// Off-screen canvas size — bigger than any real terminal, so the graph must
/// be scrolled to be seen in full.
const CANVAS_W: u16 = 220;
const CANVAS_H: u16 = 110;

// A 16-node compiler pipeline: source → parse → analyze → optimize → backend.
// Nodes vary in size (via `from_content`) on purpose.
const TITLES: [&str; 16] = [
	"src_scan", "manifest", "read", "lex", "parse", "hir", "typeck", "borrow", "lint",
	"mir", "inline", "dce", "codegen", "llvm_opt", "emit_obj", "link",
];
const CONTENTS: [&str; 16] = [
	"src_scan\nsrc/**/*.rs\nfound 42 files",
	"manifest\nCargo.toml\n142 deps",
	"read\nUTF-8 decode\n1.2M LOC",
	"lex\ntokens: 312k\nerrors: 0",
	"parse\nAST: 89k nodes\nerrors: 0",
	"lower HIR\ndesugar\nmacro expand",
	"type check\ninferred 18k\nerrors: 0",
	"borrow check\nlifetimes ok\nNLL",
	"lint\nclippy: 7 warns\nunused: 3",
	"lower MIR\ncontrol-flow\ngraph",
	"inline\nMIR opts\n12 fns inlined",
	"dead code\nremoved 240 fns\n-8% size",
	"codegen\nLLVM IR\n2.1M lines",
	"llvm opt\n-O2 passes\nLTO",
	"emit obj\nx86_64 .o\n4.8 MB",
	"link\nld.bfd\nbin 6.2 MB",
];

// (from_node, from_port, to_node, to_port) — a multi-branch / multi-join DAG.
const EDGES: [(usize, usize, usize, usize); 18] = [
	(0, 0, 2, 0),   // src_scan -> read
	(1, 0, 2, 1),   // manifest -> read
	(2, 0, 3, 0),   // read     -> lex
	(3, 0, 4, 0),   // lex      -> parse
	(4, 0, 5, 0),   // parse    -> hir
	(5, 0, 6, 0),   // hir      -> typeck
	(5, 1, 7, 0),   // hir      -> borrow
	(6, 0, 8, 0),   // typeck   -> lint
	(7, 0, 8, 1),   // borrow   -> lint
	(6, 1, 9, 0),   // typeck   -> mir
	(7, 1, 9, 1),   // borrow   -> mir
	(9, 0, 10, 0),  // mir      -> inline
	(10, 0, 11, 0), // inline  -> dce
	(8, 0, 12, 0),  // lint    -> codegen
	(11, 0, 12, 1), // dce     -> codegen
	(12, 0, 13, 0), // codegen -> llvm_opt
	(13, 0, 14, 0), // llvm_opt-> emit_obj
	(14, 0, 15, 0), // emit_obj-> link
];

const EDGE_COLORS: [Color; 6] =
	[Color::Green, Color::Yellow, Color::Blue, Color::Magenta, Color::Cyan, Color::Red];

struct App {
	graph: NodeGraph<'static>,
	state: FlowState,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let graph = build_graph();

	// enter raw mode + alternate screen; the guard restores on drop (and panic)
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen)?;
	let _guard = TerminalGuard;
	let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

	let mut app = App { graph, state: FlowState::new() };
	run_app(&mut terminal, &mut app)?;
	Ok(())
}

/// Restores the terminal even if the app panics.
struct TerminalGuard;
impl Drop for TerminalGuard {
	fn drop(&mut self) {
		let _ = disable_raw_mode();
		let _ = execute!(io::stdout(), LeaveAlternateScreen);
	}
}

/// Build the 16-node graph and run layout/routing once. The resulting off-screen
/// canvas (borders/ports/connections) lives inside the `NodeGraph`; scrolling
/// never re-runs this.
fn build_graph() -> NodeGraph<'static> {
	let nodes: Vec<NodeLayout> = TITLES
		.iter()
		.zip(CONTENTS.iter())
		.map(|(title, content)| {
			NodeLayout::from_content(content)
				.with_title(title)
				.with_border_type(BorderType::Rounded)
		})
		.collect();

	let conns: Vec<Connection> = EDGES
		.iter()
		.enumerate()
		.map(|(i, &(f, fp, t, tp))| {
			Connection::new(f.into(), fp.into(), t.into(), tp.into())
				.with_line_style(Style::default().fg(EDGE_COLORS[i % EDGE_COLORS.len()]))
		})
		.collect();

	let mut graph = NodeGraph::new(nodes, conns, CANVAS_W as usize, CANVAS_H as usize);
	graph.calculate();
	graph
}

fn run_app(
	terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
	app: &mut App,
) -> io::Result<()> {
	loop {
		terminal.draw(|f| ui(f, app))?;

		if !event::poll(Duration::from_millis(200))? {
			continue;
		}
		if let Event::Key(key) = event::read()? {
			if key.kind != KeyEventKind::Press {
				continue;
			}
			let sz = terminal.size()?;
			let view_w = sz.width;
			let view_h = sz.height.saturating_sub(1);
			let canvas = (CANVAS_W, CANVAS_H);
			match key.code {
				KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
				KeyCode::Left | KeyCode::Char('h') => {
					app.state.pan(-3, 0, canvas, (view_w, view_h))
				}
				KeyCode::Right | KeyCode::Char('l') => {
					app.state.pan(3, 0, canvas, (view_w, view_h))
				}
				KeyCode::Up | KeyCode::Char('k') => {
					app.state.pan(0, -1, canvas, (view_w, view_h))
				}
				KeyCode::Down | KeyCode::Char('j') => {
					app.state.pan(0, 1, canvas, (view_w, view_h))
				}
				KeyCode::PageUp => app.state.pan(0, -10, canvas, (view_w, view_h)),
				KeyCode::PageDown => app.state.pan(0, 10, canvas, (view_w, view_h)),
				KeyCode::Home => app.state = FlowState::new(),
				_ => {}
			}
		}
	}
}

fn ui(f: &mut Frame, app: &mut App) {
	let area = f.area();
	let status_h: u16 = 1;
	let view = Rect {
		x: area.x,
		y: area.y,
		width: area.width,
		height: area.height.saturating_sub(status_h),
	};

	// 1. node contents — screen-coordinate rects, clipped to the visible view
	let bold = Style::default().add_modifier(Modifier::BOLD);
	let zones = app.graph.split_stateful(view, &app.state);
	for (id, z) in &zones {
		if z.width > 0 && z.height > 0 && (id.as_u32() as usize) < CONTENTS.len() {
			f.render_widget(
				Paragraph::new(CONTENTS[id.as_u32() as usize]).style(bold),
				*z,
			);
		}
	}

	// 2. borders / ports / connections (blit) + selection/hover highlight.
	//    The graph is consumed by render_stateful_widget, so we clone-free by
	//    rebuilding only if needed — here we render once per frame via the
	//    stateful path. `app.graph` is borrowed; we render a clone-by-value
	//    NodeGraph only when the borrow-checker demands it. To keep this
	//    example allocation-free, we render into the frame directly using the
	//    graph's stateful impl on a freshly-borrowed copy via clone.
	let mut state = app.state.clone();
	f.render_stateful_widget(app.graph.clone(), view, &mut state);

	// status bar (stays fixed regardless of scroll)
	let status = Rect {
		x: area.x,
		y: view.bottom(),
		width: area.width,
		height: status_h,
	};
	let msg = format!(
		" view_offset=({}, {})  canvas={}x{}  \u{2190}\u{2192}\u{2191}\u{2193}/hjkl scroll · PgUp/PgDn · Home=reset · q/Esc=quit ",
		app.state.view_offset.0, app.state.view_offset.1, CANVAS_W, CANVAS_H
	);
	f.render_widget(
		Paragraph::new(msg).style(Style::default().add_modifier(Modifier::REVERSED)),
		status,
	);
}
