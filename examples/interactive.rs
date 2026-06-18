//! Interactive TUI end-to-end demo of the Step 6 stateful path:
//!   - **mouse move** → `hover` the node under the cursor (cyan-ish border);
//!   - **mouse click** → `select` the node (bold yellow border);
//!   - **hjkl / arrows** → `pan` the viewport (clamped to the canvas bounds);
//!   - **q / Esc** → quit.
//!
//! Everything runs through `FlowState`: `hit_test` maps a screen point to a
//! `NodeId`, `split_stateful` yields per-node content rects under the current
//! pan, and `render_stateful_widget` blits the scrolled canvas + overlays the
//! selection/hover highlight.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
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

/// Off-screen canvas size — bigger than a typical terminal so panning matters.
const CANVAS_W: u16 = 220;
const CANVAS_H: u16 = 110;

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

const EDGES: [(usize, usize, usize, usize); 18] = [
	(0, 0, 2, 0),
	(1, 0, 2, 1),
	(2, 0, 3, 0),
	(3, 0, 4, 0),
	(4, 0, 5, 0),
	(5, 0, 6, 0),
	(5, 1, 7, 0),
	(6, 0, 8, 0),
	(7, 0, 8, 1),
	(6, 1, 9, 0),
	(7, 1, 9, 1),
	(9, 0, 10, 0),
	(10, 0, 11, 0),
	(8, 0, 12, 0),
	(11, 0, 12, 1),
	(12, 0, 13, 0),
	(13, 0, 14, 0),
	(14, 0, 15, 0),
];

const EDGE_COLORS: [Color; 6] =
	[Color::Green, Color::Yellow, Color::Blue, Color::Magenta, Color::Cyan, Color::Red];

struct App {
	graph: NodeGraph<'static>,
	state: FlowState,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let graph = build_graph();

	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen)?;
	// enable mouse tracking so we get move/click events
	execute!(stdout, crossterm::event::EnableMouseCapture)?;
	let _guard = TerminalGuard;
	let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

	let mut app = App { graph, state: FlowState::new() };
	run_app(&mut terminal, &mut app)?;
	Ok(())
}

/// Restores the terminal (and disables mouse capture) even on panic.
struct TerminalGuard;
impl Drop for TerminalGuard {
	fn drop(&mut self) {
		let _ = execute!(io::stdout(), crossterm::event::DisableMouseCapture);
		let _ = disable_raw_mode();
		let _ = execute!(io::stdout(), LeaveAlternateScreen);
	}
}

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
	// Make the selection highlight pop: bold magenta (hover gets +DIM).
	graph = graph.highlight_style(
		Style::default().add_modifier(Modifier::BOLD).fg(Color::Magenta),
	);
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
		match event::read()? {
			Event::Key(key) => {
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
					KeyCode::Home => app.state = FlowState::new(),
					_ => {}
				}
			}
			Event::Mouse(mouse) => {
				// view area excludes the 1-row status bar at the bottom
				let sz = terminal.size()?;
				let view = Rect {
					x: 0,
					y: 0,
					width: sz.width,
					height: sz.height.saturating_sub(1),
				};
				// hit_test takes screen coords within `view`; account for the pan
				// by translating the screen point into canvas-relative coords
				// (hit_test itself is pan-unaware, so we pre-add the offset).
				let (ox, oy) = app.state.view_offset;
				let cx = mouse.column.saturating_add(ox);
				let cy = mouse.row.saturating_add(oy);
				// hit_test interprets (x,y) relative to `view`'s origin and uses
				// the graph's canvas-space placements, so feed it canvas coords
				// while keeping `view` as the frame of reference (origin 0,0).
				let hit =
					app.graph.hit_test(Rect::new(0, 0, view.width, view.height), cx, cy);
				match mouse.kind {
					MouseEventKind::Moved => app.state.hover = hit,
					MouseEventKind::Down(MouseButton::Left) => app.state.selection = hit,
					_ => {}
				}
			}
			_ => {}
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

	// 1. node contents via split_stateful (pan-aware content rects)
	let bold = Style::default().add_modifier(Modifier::BOLD);
	let zones = app.graph.split_stateful(view, &app.state);
	for (id, z) in &zones {
		let idx = id.as_u32() as usize;
		if z.width > 0 && z.height > 0 && idx < CONTENTS.len() {
			f.render_widget(Paragraph::new(CONTENTS[idx]).style(bold), *z);
		}
	}

	// 2. blit + highlight via the stateful path
	let mut state = app.state.clone();
	f.render_stateful_widget(app.graph.clone(), view, &mut state);

	// status bar
	let status = Rect {
		x: area.x,
		y: view.bottom(),
		width: area.width,
		height: status_h,
	};
	let sel =
		app.state.selection.map(|i| format!("{}", i)).unwrap_or_else(|| "none".into());
	let hov = app.state.hover.map(|i| format!("{}", i)).unwrap_or_else(|| "none".into());
	let msg = format!(
		" offset=({}, {})  sel={}  hover={}  mouse=move/click  hjkl/arrows=pan  q=quit ",
		app.state.view_offset.0, app.state.view_offset.1, sel, hov,
	);
	f.render_widget(
		Paragraph::new(msg).style(Style::default().add_modifier(Modifier::REVERSED)),
		status,
	);
}
