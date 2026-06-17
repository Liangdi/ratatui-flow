//! Interactive TUI: a complex compiler-pipeline node graph that is bigger
//! than the screen, with keyboard-driven viewport scrolling.
//!
//! This uses the library's native viewport API: the whole graph is laid out
//! **once** into an off-screen canvas (during `NodeGraph::calculate`), and each
//! frame we:
//!   1. ask the graph for each node's *screen-coordinate* content rect via
//!      `split_viewport`, and render our own content widget into it;
//!   2. render the `NodeGraphView` widget, which blits the scrolled window of
//!      the canvas (borders / ports / connections) onto the terminal.
//!
//! Scrolling is just changing the `Viewport` offset and redrawing; the
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
	viewport: Viewport,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let graph = build_graph();

	// enter raw mode + alternate screen; the guard restores on drop (and panic)
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen)?;
	let _guard = TerminalGuard;
	let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

	let mut app = App { graph, viewport: Viewport::new() };
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
			let max = max_scroll((sz.width, sz.height), (CANVAS_W, CANVAS_H));
			match key.code {
				KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
				KeyCode::Left | KeyCode::Char('h') => {
					app.viewport.offset.0 = app.viewport.offset.0.saturating_sub(3)
				}
				KeyCode::Right | KeyCode::Char('l') => {
					app.viewport.offset.0 =
						app.viewport.offset.0.saturating_add(3).min(max.0)
				}
				KeyCode::Up | KeyCode::Char('k') => {
					app.viewport.offset.1 = app.viewport.offset.1.saturating_sub(1)
				}
				KeyCode::Down | KeyCode::Char('j') => {
					app.viewport.offset.1 =
						app.viewport.offset.1.saturating_add(1).min(max.1)
				}
				KeyCode::PageUp => {
					app.viewport.offset.1 = app.viewport.offset.1.saturating_sub(10)
				}
				KeyCode::PageDown => {
					app.viewport.offset.1 =
						app.viewport.offset.1.saturating_add(10).min(max.1)
				}
				KeyCode::Home => app.viewport = Viewport::new(),
				_ => {}
			}
		}
	}
}

/// Largest scroll offset that still keeps the viewport on the canvas, with the
/// status bar row reserved.
fn max_scroll(screen: (u16, u16), canvas: (u16, u16)) -> (u16, u16) {
	let view_w = screen.0;
	let view_h = screen.1.saturating_sub(1);
	(canvas.0.saturating_sub(view_w), canvas.1.saturating_sub(view_h))
}

fn ui(f: &mut Frame, app: &App) {
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
	let zones = app.graph.split_viewport(view, &app.viewport);
	for (i, z) in zones.iter().enumerate() {
		if i < CONTENTS.len() && z.width > 0 && z.height > 0 {
			f.render_widget(Paragraph::new(CONTENTS[i]).style(bold), *z);
		}
	}

	// 2. borders / ports / connections — blit the scrolled window of the canvas
	f.render_widget(NodeGraphView::new(&app.graph).viewport(app.viewport), view);

	// status bar (stays fixed regardless of scroll)
	let status = Rect {
		x: area.x,
		y: view.bottom(),
		width: area.width,
		height: status_h,
	};
	let msg = format!(
		" viewport=({}, {})  canvas={}x{}  \u{2190}\u{2192}\u{2191}\u{2193}/hjkl scroll · PgUp/PgDn · Home=reset · q/Esc=quit ",
		app.viewport.offset.0, app.viewport.offset.1, CANVAS_W, CANVAS_H
	);
	f.render_widget(
		Paragraph::new(msg).style(Style::default().add_modifier(Modifier::REVERSED)),
		status,
	);
}
