//! Interactive flow-graph **editor** built on `ratatui-flow`.
//!
//! A small data-pipeline canvas you can edit live:
//!   - **hjkl / arrows / wheel** — pan the canvas;
//!   - **mouse move / click** — hover / select nodes (click also toggles a link
//!     while in CONNECT mode);
//!   - **Tab / Shift-Tab** — cycle selection through nodes;
//!   - **n** (or **2**) — add a `Process` node upstream of the selection;
//!   - **1 / 3 / 4** — add a `Source` / `Filter` / `Sink` node;
//!   - **d** (or **Delete**) — delete the selected node (connections cascade);
//!   - **c** — toggle CONNECT mode: pick a target with click / Tab+Enter to
//!     toggle a link from the source (cycles & duplicates are guarded);
//!   - **r** — rotate the flow direction (Rtl → Ltr → Ttb → Btt);
//!   - **a** — toggle connection direction arrows;
//!   - **t** — cycle the sci-fi theme (DeepSpace → Cyberpunk → …);
//!   - **p** — toggle the side panel; **?** — help; **q / Esc** — quit.
//!
//! The graph is the single source of truth: nodes/connections are mutated in
//! place via `add_node` / `remove_node` / `add_connection` / `remove_connection`
//! and re-laid-out with `calculate` before each draw. Node *content* lives in
//! the app (the framework only stores size/title) and is rendered into the
//! rects from `split_stateful`.

use std::collections::HashMap as Map;
use std::collections::HashSet as Set;
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{
	self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
	EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::{
	Frame, Terminal,
	backend::CrosstermBackend,
	buffer::Buffer,
	layout::{Position, Rect},
	style::{Color, Modifier, Style},
	widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use ratatui_flow::*;
use ratatui_sci_fi::{Palette, Theme};

/// Off-screen canvas size — larger than a typical terminal so panning matters,
/// but kept modest so the initial pipeline lands inside the default view under
/// the canvas-absolute render (add nodes / pan to explore beyond it).
const CANVAS_W: u16 = 120;
const CANVAS_H: u16 = 50;
/// Bottom status bar height (mode/message + keymap hint).
const STATUS_H: u16 = 2;
/// Width of the collapsible left side panel.
const PANEL_W: u16 = 26;

// Node/edge colors are derived from the active sci-fi theme's palette (see
// `App::pal` / `App::edge_color`), so the whole editor recolors together when
// the theme is cycled with `t`.

// --- node kinds ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
	Source,
	Process,
	Filter,
	Sink,
}

impl Kind {
	fn label(self) -> &'static str {
		match self {
			Kind::Source => "src",
			Kind::Process => "proc",
			Kind::Filter => "filt",
			Kind::Sink => "sink",
		}
	}

	/// Theme-aware color for this kind: each kind maps to a semantic palette
	/// slot, so the whole graph recolors when the theme (`t`) changes.
	fn color(self, pal: Palette) -> Color {
		match self {
			Kind::Source => pal.ok.color(),
			Kind::Process => pal.accent.color(),
			Kind::Filter => pal.warn.color(),
			Kind::Sink => pal.accent2.color(),
		}
	}

	/// Multi-line body shown inside the node. The node's size is derived from
	/// this so the border fits it exactly.
	fn body(self, n: usize) -> String {
		match self {
			Kind::Source => format!("source\nread input\nstream #{n}"),
			Kind::Process => format!("process\nmap rows\nbatch #{n}"),
			Kind::Filter => format!("filter\nwhere p(x)\npass #{n}"),
			Kind::Sink => format!("sink\nwrite out\nrows #{n}"),
		}
	}
}

struct NodeInfo {
	#[allow(dead_code)]
	id: NodeId,
	kind: Kind,
	title: String,
	body: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
	Normal,
	/// Pick a target to toggle a link from `connect_from`.
	Connect,
}

/// In-progress left-button drag of a node: which node, plus the offset from the
/// cursor's canvas position to the node's top-left at grab time (so the node
/// follows the cursor without snapping its corner to it).
struct DragState {
	node: NodeId,
	/// Canvas-space offset: `(cursor - node_top_left)` at grab time.
	dx: i32,
	dy: i32,
}

struct App {
	graph: NodeGraph<'static>,
	/// App-side content per node (the framework stores size/title only).
	info: Map<NodeId, NodeInfo>,
	state: FlowState,
	mode: Mode,
	/// Active left-button node drag; `None` when idle.
	drag: Option<DragState>,
	/// Source node of the in-progress link (CONNECT mode).
	connect_from: Option<NodeId>,
	/// Active sci-fi theme (cycle with `t`); drives every color below.
	theme: Theme,
	direction: FlowDirection,
	show_arrows: bool,
	show_panel: bool,
	show_help: bool,
	/// Monotonic serial baked into node titles/bodies.
	serial: usize,
	message: String,
	msg_at: Instant,
	quit: bool,
}

impl App {
	fn new() -> Self {
		// Selection highlight: bold theme accent (hover gets +DIM from the
		// framework). Pinned mode: freshly added nodes auto-layout, but any node
		// the user drags (via `set_position` below) is treated as an immovable
		// anchor and keeps its place across `calculate()`.
		let theme = Theme::DeepSpace;
		let pal = theme.palette();
		let graph = NodeGraph::new(vec![], vec![], CANVAS_W as usize, CANVAS_H as usize)
			.with_layout_mode(LayoutMode::Pinned)
			.highlight_style(
				Style::default().add_modifier(Modifier::BOLD).fg(pal.accent.color()),
			);
		let mut app = Self {
			graph,
			info: Map::new(),
			state: FlowState::new(),
			mode: Mode::Normal,
			drag: None,
			connect_from: None,
			theme,
			direction: FlowDirection::Rtl,
			show_arrows: false,
			show_panel: true,
			show_help: false,
			serial: 0,
			message: String::from("welcome — press ? for help"),
			msg_at: Instant::now(),
			quit: false,
		};
		app.seed();
		app.graph.calculate();
		// select the first source so something is highlighted on start (it sits
		// on the visible left edge under the default Rtl layout).
		app.state.selection = app.graph.nodes().first().map(|(id, _)| *id);
		app
	}

	/// Build the initial demo pipeline: src → parse/valid → xform → filter/sink.
	fn seed(&mut self) {
		let kinds = [
			Kind::Source,
			Kind::Process,
			Kind::Filter,
			Kind::Process,
			Kind::Filter,
			Kind::Sink,
		];
		let ids: Vec<NodeId> = kinds.iter().map(|&k| self.add_node(k)).collect();
		// `from` = source/producer, `to` = consumer (sinks toward the root edge).
		let edges = [(0, 1), (0, 2), (1, 3), (2, 3), (3, 4), (3, 5), (4, 5)];
		for (a, b) in edges {
			self.link_color(ids[a], ids[b]);
		}
	}

	// --- mutations ---------------------------------------------------------

	/// Append a node of `kind`, returning its id and recording its content.
	fn add_node(&mut self, kind: Kind) -> NodeId {
		self.serial += 1;
		let n = self.serial;
		let title = format!("{}:{}", kind.label(), n);
		let body = kind.body(n);
		let size = fit_size(&body);
		let layout = NodeLayout::new(size)
			.with_title(leak(title.clone()))
			.with_border_type(BorderType::Rounded)
			.with_border_style(Style::default().fg(kind.color(self.pal())));
		let id = self.graph.add_node(layout);
		self.info.insert(id, NodeInfo { id, kind, title, body });
		id
	}

	/// Add a `Process` node upstream of the selection (it feeds the selected
	/// node), then select the new node. With no selection, an isolated node is
	/// added.
	fn do_add(&mut self, kind: Kind) {
		let id = self.add_node(kind);
		if let Some(sel) = self.state.selection
			&& sel != id
		{
			// new node is the source, selected node is the sink — the new
			// node lands on the open (source) side, so it stays on-screen.
			self.link_color(id, sel);
		}
		self.state.selection = Some(id);
		self.flash(format!("added {} (#{})", kind.label(), self.serial));
	}

	fn do_delete(&mut self) {
		let Some(sel) = self.state.selection else {
			self.flash("nothing selected");
			return;
		};
		if self.graph.remove_node(sel) {
			self.info.remove(&sel);
			if self.state.hover == Some(sel) {
				self.state.hover = None;
			}
			if self.connect_from == Some(sel) {
				self.mode = Mode::Normal;
				self.connect_from = None;
			}
			self.state.selection = None;
			self.flash(format!("deleted node #{}", sel.as_u32()));
		}
	}

	/// Append a colored connection `from` → `to` (port 0 on both ends).
	fn link_color(&mut self, from: NodeId, to: NodeId) {
		let color = self.edge_color(self.graph.connections().len());
		let style = Style::default().fg(color);
		self.graph.add_connection(
			Connection::new(from, 0usize.into(), to, 0usize.into())
				.with_line_style(style),
		);
	}

	/// Toggle CONNECT mode (uses the current selection as the link source).
	fn do_connect(&mut self) {
		match self.mode {
			Mode::Normal => {
				let Some(sel) = self.state.selection else {
					self.flash("select a source node first");
					return;
				};
				self.connect_from = Some(sel);
				self.mode = Mode::Connect;
				self.flash("CONNECT: pick target (click / Tab+Enter) — toggles link");
			}
			Mode::Connect => {
				self.mode = Mode::Normal;
				self.connect_from = None;
				self.flash("connect cancelled");
			}
		}
	}

	/// Toggle the link `connect_from` → `target` (remove if present, else add,
	/// refusing cycles). Only meaningful in CONNECT mode.
	fn do_link_target(&mut self, target: NodeId) {
		let Some(src) = self.connect_from else { return };
		if src == target {
			self.flash("target is the source — skipped");
			return;
		}
		let exists = self.graph.has_connection(src, target);
		if exists {
			let _ =
				self.graph.remove_connection(src, 0usize.into(), target, 0usize.into());
			self.flash(format!("removed link {} -> {}", src.as_u32(), target.as_u32()));
		} else if self.reaches(target, src) {
			// adding src->to would close a cycle (to already reaches src).
			self.flash("cycle detected — link refused");
		} else {
			self.link_color(src, target);
			self.flash(format!("linked {} -> {}", src.as_u32(), target.as_u32()));
		}
	}

	/// `true` if `target` is reachable from `start` following `from -> to` edges.
	/// Used to keep the graph acyclic when adding links.
	fn reaches(&self, start: NodeId, target: NodeId) -> bool {
		let mut stack = vec![start];
		let mut seen = Set::new();
		while let Some(cur) = stack.pop() {
			if cur == target {
				return true;
			}
			if !seen.insert(cur) {
				continue;
			}
			for c in self.graph.connections() {
				if c.from_node() == cur {
					stack.push(c.to_node());
				}
			}
		}
		false
	}

	/// Cycle selection through nodes (render/insertion order).
	fn cycle_sel(&mut self, fwd: bool) {
		let ids: Vec<NodeId> = self.graph.nodes().iter().map(|(id, _)| *id).collect();
		if ids.is_empty() {
			return;
		}
		let next = match self.state.selection {
			None => ids[if fwd { 0 } else { ids.len() - 1 }],
			Some(cur) => {
				let i = ids.iter().position(|x| *x == cur).unwrap_or(0);
				let n = ids.len() as isize;
				let step = if fwd { 1 } else { -1 };
				ids[((i as isize + step).rem_euclid(n)) as usize]
			}
		};
		self.state.selection = Some(next);
	}

	fn rotate_direction(&mut self) {
		self.direction = next_dir(self.direction);
		self.graph.set_direction(self.direction);
		self.flash(format!("direction: {:?}", self.direction));
	}

	fn toggle_arrows(&mut self) {
		self.show_arrows = !self.show_arrows;
		self.graph.set_show_arrows(self.show_arrows);
		self.flash(if self.show_arrows { "arrows on" } else { "arrows off" });
	}

	/// Active theme's palette — the single source of color for the editor.
	fn pal(&self) -> Palette {
		self.theme.palette()
	}

	/// Edge color for the `idx`-th connection (insertion order), cycled through
	/// palette slots so connections recolor with the theme.
	fn edge_color(&self, idx: usize) -> Color {
		let p = self.pal();
		[p.accent, p.accent2, p.ok, p.warn, p.alert, p.muted][idx % 6].color()
	}

	fn rotate_theme(&mut self) {
		self.theme = next_theme(self.theme);
		let accent = self.pal().accent.color();
		// The framework applies `highlight_style` for selection/hover at render
		// time, so this takes effect next frame (no `calculate` needed).
		self.graph.set_highlight_style(
			Style::default().add_modifier(Modifier::BOLD).fg(accent),
		);
		self.flash(format!("theme: {:?}", self.theme));
	}

	fn flash(&mut self, msg: impl Into<String>) {
		self.message = msg.into();
		self.msg_at = Instant::now();
	}

	/// Pan minimally so the current selection stays in view (uses the new
	/// `FlowState::ensure_visible` + `NodeGraph::node_canvas_rect`). Called after
	/// selection-changing keys so Tab/BackTab/add never leave the highlighted
	/// node off-screen.
	fn bring_selection_into_view(&mut self, view: Rect) {
		if let Some(sel) = self.state.selection
			&& let Some(rect) = self.graph.node_canvas_rect(sel)
		{
			self.state.ensure_visible(
				rect,
				(CANVAS_W, CANVAS_H),
				(view.width, view.height),
			);
		}
	}

	// --- input -------------------------------------------------------------

	fn on_key(&mut self, key: KeyEvent, size: Rect) {
		if key.kind == crossterm::event::KeyEventKind::Release {
			return;
		}
		// Ignore Ctrl-combos so Ctrl+C etc. don't masquerade as plain chars.
		if key.modifiers.contains(KeyModifiers::CONTROL) {
			return;
		}
		let view = graph_area(self.show_panel, size);
		let canvas = (CANVAS_W, CANVAS_H);
		let view_size = (view.width, view.height);
		match key.code {
			KeyCode::Char('q') | KeyCode::Esc => {
				if self.mode == Mode::Connect {
					self.mode = Mode::Normal;
					self.connect_from = None;
					self.flash("connect cancelled");
				} else if self.show_help {
					self.show_help = false;
				} else {
					self.quit = true;
				}
			}
			KeyCode::Char('?') => self.show_help = !self.show_help,
			KeyCode::Char('p') => {
				self.show_panel = !self.show_panel;
				self.flash(if self.show_panel { "panel on" } else { "panel off" });
			}
			KeyCode::Char('r') => self.rotate_direction(),
			KeyCode::Char('a') => self.toggle_arrows(),
			KeyCode::Char('t') => self.rotate_theme(),
			KeyCode::Char('n') | KeyCode::Char('2') => {
				self.do_add(Kind::Process);
				self.bring_selection_into_view(view);
			}
			KeyCode::Char('1') => {
				self.do_add(Kind::Source);
				self.bring_selection_into_view(view);
			}
			KeyCode::Char('3') => {
				self.do_add(Kind::Filter);
				self.bring_selection_into_view(view);
			}
			KeyCode::Char('4') => {
				self.do_add(Kind::Sink);
				self.bring_selection_into_view(view);
			}
			KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete => self.do_delete(),
			KeyCode::Char('c') => self.do_connect(),
			KeyCode::Tab => {
				self.cycle_sel(true);
				self.bring_selection_into_view(view);
			}
			KeyCode::BackTab => {
				self.cycle_sel(false);
				self.bring_selection_into_view(view);
			}
			KeyCode::Enter => {
				if self.mode == Mode::Connect
					&& let Some(t) = self.state.selection
				{
					self.do_link_target(t);
				}
			}
			KeyCode::Char('h') | KeyCode::Left => {
				self.state.pan(-4, 0, canvas, view_size)
			}
			KeyCode::Char('l') | KeyCode::Right => {
				self.state.pan(4, 0, canvas, view_size)
			}
			KeyCode::Char('k') | KeyCode::Up => self.state.pan(0, -2, canvas, view_size),
			KeyCode::Char('j') | KeyCode::Down => self.state.pan(0, 2, canvas, view_size),
			KeyCode::Home => {
				self.state = FlowState::new();
				self.flash("view reset");
			}
			_ => {}
		}
	}

	fn on_mouse(&mut self, m: MouseEvent, size: Rect) {
		let view = graph_area(self.show_panel, size);
		if m.column < view.x
			|| m.row < view.y
			|| m.column >= view.right()
			|| m.row >= view.bottom()
		{
			return;
		}
		let canvas = (CANVAS_W, CANVAS_H);
		let view_size = (view.width, view.height);
		// Translate screen → canvas coords (hit_test is pan-unaware, so we
		// pre-add the offset), matching the interactive.rs reference.
		let (ox, oy) = self.state.view_offset;
		let cx = m.column.saturating_sub(view.x).saturating_add(ox);
		let cy = m.row.saturating_sub(view.y).saturating_add(oy);
		let hit = self.graph.hit_test(Rect::new(0, 0, view.width, view.height), cx, cy);
		match m.kind {
			MouseEventKind::Moved => self.state.hover = hit,
			MouseEventKind::Down(MouseButton::Left) => {
				if self.mode == Mode::Connect
					&& let Some(t) = hit
				{
					self.do_link_target(t);
					self.state.selection = Some(t);
				} else if let Some(t) = hit {
					// Begin a drag if we hit a node: record the offset from the
					// cursor to the node's current top-left so it follows the
					// cursor without snapping its corner onto it.
					self.state.selection = Some(t);
					if let Some(r) = self.graph.node_rect(t) {
						let dx = cx as i32 - r.x as i32;
						let dy = cy as i32 - r.y as i32;
						self.drag = Some(DragState { node: t, dx, dy });
					}
				} else {
					self.state.selection = hit;
				}
			}
			MouseEventKind::Drag(MouseButton::Left) => {
				// Persist the drag into the graph so the next `calculate()`
				// keeps the node here (Pinned mode treats set_position entries
				// as immovable anchors). Clamp to the non-negative canvas.
				if let Some(d) = &self.drag {
					let nx = (cx as i32 - d.dx).max(0) as u16;
					let ny = (cy as i32 - d.dy).max(0) as u16;
					self.graph.set_position(d.node, nx, ny);
				}
			}
			MouseEventKind::Up(MouseButton::Left) => self.drag = None,
			MouseEventKind::ScrollUp => self.state.pan(0, -3, canvas, view_size),
			MouseEventKind::ScrollDown => self.state.pan(0, 3, canvas, view_size),
			_ => {}
		}
	}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen)?;
	execute!(stdout, crossterm::event::EnableMouseCapture)?;
	let _guard = TerminalGuard;
	let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

	let mut app = App::new();
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

fn run_app(
	terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
	app: &mut App,
) -> io::Result<()> {
	while !app.quit {
		terminal.draw(|f| ui(f, app))?;

		// expire the flash message after a few seconds.
		if !app.message.is_empty() && app.msg_at.elapsed() > Duration::from_secs(3) {
			app.message.clear();
		}

		if !event::poll(Duration::from_millis(100))? {
			continue;
		}
		match event::read()? {
			Event::Key(k) => app.on_key(k, terminal.size()?.into()),
			Event::Mouse(m) => app.on_mouse(m, terminal.size()?.into()),
			_ => {}
		}
	}
	Ok(())
}

// --- rendering ----------------------------------------------------------------

fn ui(f: &mut Frame, app: &mut App) {
	if app.graph.is_dirty() {
		app.graph.calculate();
	}
	let area = f.area();
	// Base: fill the whole frame with the theme background so the active
	// theme's color shows through the graph's blank canvas cells.
	f.buffer_mut().set_style(area, Style::default().bg(app.pal().bg.color()));
	let view = graph_area(app.show_panel, area);
	if app.show_panel {
		let panel = Rect {
			x: area.x,
			y: area.y,
			width: PANEL_W.min(area.width),
			height: area.height.saturating_sub(STATUS_H),
		};
		draw_panel(f, app, panel);
	}
	draw_graph(f, app, view);
	draw_status(f, app, area);
	if app.show_help {
		draw_help(f, app, area);
	}
}

fn draw_graph(f: &mut Frame, app: &mut App, area: Rect) {
	let pal = app.pal();
	// 1. node content via the pan-aware content rects.
	let zones = app.graph.split_stateful(area, &app.state);
	for (id, rect) in &zones {
		if rect.width > 0
			&& rect.height > 0
			&& let Some(info) = app.info.get(id)
		{
			f.render_widget(
				Paragraph::new(info.body.as_str()).style(
					Style::default()
						.fg(info.kind.color(pal))
						.add_modifier(Modifier::BOLD),
				),
				*rect,
			);
		}
	}

	// 2. borders / ports / connections / selection+hover highlight.
	let mut state = app.state.clone();
	f.render_stateful_widget(app.graph.clone(), area, &mut state);

	// 3. CONNECT mode: recolor the source node's border green so the link's
	//    origin reads at a glance (selection stays on the target candidate).
	if app.mode == Mode::Connect
		&& let Some(src) = app.connect_from
	{
		for (id, rect) in &zones {
			if *id == src && rect.width > 0 && rect.height > 0 {
				let style =
					Style::default().fg(pal.ok.color()).add_modifier(Modifier::BOLD);
				paint_border(f.buffer_mut(), *rect, style);
				break;
			}
		}
	}
}

fn draw_panel(f: &mut Frame, app: &App, area: Rect) {
	let pal = app.pal();
	let block = Block::default()
		.borders(Borders::ALL)
		.border_style(Style::default().fg(pal.muted.color()))
		.style(Style::default().bg(pal.panel.color()))
		.title(" graph ")
		.title_style(
			Style::default().fg(pal.accent.color()).add_modifier(Modifier::BOLD),
		);
	let inner = block.inner(area);
	f.render_widget(block, area);
	if inner.width < 4 {
		return;
	}

	let mut s = String::new();
	s.push_str(&format!("nodes : {}\n", app.graph.nodes().len()));
	s.push_str(&format!("edges : {}\n", app.graph.connections().len()));
	s.push_str(&format!("dir   : {:?}\n", app.direction));
	if app.show_arrows {
		s.push_str("arrows: on\n");
	}
	s.push('\n');

	// one line per node; the selection is marked with ▶.
	let cap = (inner.width as usize).saturating_sub(7);
	for (id, layout) in app.graph.nodes() {
		let marker = if app.state.selection == Some(*id) { "▶" } else { " " };
		let tag = app.info.get(id).map(|i| i.kind.label()).unwrap_or("?");
		let title =
			app.info.get(id).map(|i| i.title.as_str()).unwrap_or_else(|| layout.title());
		s.push_str(&format!("{marker}{tag:<4} {}\n", truncate(title, cap)));
	}

	f.render_widget(Paragraph::new(s).style(Style::default().fg(pal.fg.color())), inner);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
	let pal = app.pal();
	let top = Rect {
		x: area.x,
		y: area.bottom().saturating_sub(STATUS_H),
		width: area.width,
		height: 1,
	};
	let bottom = Rect {
		x: area.x,
		y: top.y + 1,
		width: area.width,
		height: 1,
	};

	let sel = app
		.state
		.selection
		.map(|i| i.as_u32().to_string())
		.unwrap_or_else(|| "-".to_string());
	let (ox, oy) = app.state.view_offset;
	let mode = match app.mode {
		Mode::Normal => "NORMAL",
		Mode::Connect => "CONNECT",
	};
	let mut line = format!(" [{mode}]  off=({ox},{oy})  sel={sel} ");
	if app.mode == Mode::Connect {
		let src = app
			.connect_from
			.map(|i| i.as_u32().to_string())
			.unwrap_or_else(|| "-".to_string());
		line.push_str(&format!(" src={src}  pick target · c=cancel "));
	} else if !app.message.is_empty() {
		line.push_str(&format!(" {} ", app.message));
	}
	f.render_widget(
		Paragraph::new(line).style(Style::default().add_modifier(Modifier::REVERSED)),
		top,
	);

	let hints = "? help · n/2 proc · 1 src · 3 filt · 4 sink · d del · c link · \
	             tab cycle · r rotate · a arrows · t theme · p panel · hjkl/wheel pan · q quit";
	f.render_widget(
		Paragraph::new(hints).style(Style::default().fg(pal.muted.color())),
		bottom,
	);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
	let pal = app.pal();
	const W: u16 = 48;
	const H: u16 = 20;
	let x = area.x + (area.width.saturating_sub(W)) / 2;
	let y = area.y + (area.height.saturating_sub(H)) / 2;
	let r = Rect { x, y, width: W, height: H };
	f.render_widget(Clear, r);

	let text = "\
Flow editor — ratatui-flow

  hjkl / arrows   pan canvas
  scroll wheel    pan vertically
  mouse move      hover node
  click           select (link target in CONNECT)
  Tab / Shift+Tab cycle selection
  n  or  2        add Process node
  1 / 3 / 4       add Source / Filter / Sink
  d  or  Delete   delete selected node
  c               CONNECT mode (toggle links)
  Enter           confirm link (CONNECT)
  r               rotate flow direction
  a               toggle direction arrows
  t               cycle sci-fi theme
  p               toggle side panel
  Home            reset view
  q  /  Esc       quit
";
	let block = Block::default()
		.borders(Borders::ALL)
		.border_type(BorderType::Rounded)
		.border_style(
			Style::default().fg(pal.accent.color()).add_modifier(Modifier::BOLD),
		)
		.title(" help — ? or Esc to close ");
	f.render_widget(
		Paragraph::new(text).style(Style::default().fg(pal.fg.color())),
		block.inner(r),
	);
	f.render_widget(block, r);
}

// --- helpers ------------------------------------------------------------------

/// Graph viewport rect: the area above the status bar, minus the side panel
/// when it is open.
fn graph_area(show_panel: bool, area: Rect) -> Rect {
	let height = area.height.saturating_sub(STATUS_H);
	if show_panel && area.width > PANEL_W {
		Rect {
			x: area.x + PANEL_W,
			y: area.y,
			width: area.width - PANEL_W,
			height,
		}
	} else {
		Rect { x: area.x, y: area.y, width: area.width, height }
	}
}

/// Node size (border included) that exactly fits `body`'s widest line + lines.
fn fit_size(body: &str) -> (u16, u16) {
	let w = body.lines().map(|l| l.chars().count()).max().unwrap_or(0);
	let h = body.lines().count();
	((w as u16) + 2, (h as u16) + 2)
}

/// Recolor the perimeter of the bordered frame around an inner content `rect`
/// without touching the glyphs the canvas already placed there (so port symbols
/// survive). Used for the CONNECT-mode source highlight.
fn paint_border(buf: &mut Buffer, inner: Rect, style: Style) {
	let x0 = inner.x.saturating_sub(1);
	let y0 = inner.y.saturating_sub(1);
	let x1 = inner.right();
	let y1 = inner.bottom();
	for x in x0..=x1 {
		merge(buf, x, y0, style);
		merge(buf, x, y1, style);
	}
	for y in y0..=y1 {
		merge(buf, x0, y, style);
		merge(buf, x1, y, style);
	}
}

fn merge(buf: &mut Buffer, x: u16, y: u16, style: Style) {
	if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
		cell.set_style(cell.style().patch(style));
	}
}

/// Truncate `s` to `n` display cells, appending an ellipsis when it overflows.
fn truncate(s: &str, n: usize) -> String {
	if n == 0 {
		return String::new();
	}
	if s.chars().count() <= n {
		return s.to_string();
	}
	let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
	t.push('…');
	t
}

/// Promote an owned `String` to a `&'static str` so it can back a
/// `NodeLayout<'static>` title. Nodes are added infrequently (user actions), so
/// the leak is bounded and acceptable for an example.
fn leak(s: String) -> &'static str {
	Box::leak(s.into_boxed_str())
}

fn next_dir(d: FlowDirection) -> FlowDirection {
	use FlowDirection as D;
	match d {
		D::Rtl => D::Ltr,
		D::Ltr => D::Ttb,
		D::Ttb => D::Btt,
		D::Btt => D::Rtl,
	}
}

/// Cycle through the eight built-in sci-fi themes.
fn next_theme(t: Theme) -> Theme {
	use Theme as T;
	match t {
		T::Cyberpunk => T::Fallout,
		T::Fallout => T::Weyland,
		T::Weyland => T::DeepSpace,
		T::DeepSpace => T::Bloodmoon,
		T::Bloodmoon => T::Nebula,
		T::Nebula => T::Arctic,
		T::Arctic => T::Sentinel,
		T::Sentinel => T::Cyberpunk,
	}
}

// Deterministic checks of the editing logic (driven directly, no terminal).
// `cargo test --example flow_editor` runs these.
#[cfg(test)]
mod tests {
	use super::*;
	use ratatui::widgets::StatefulWidget;

	fn nid(n: u32) -> NodeId {
		n.into()
	}

	#[test]
	fn seed_has_six_nodes_and_seven_edges() {
		let app = App::new();
		assert_eq!(app.graph.nodes().len(), 6);
		assert_eq!(app.graph.connections().len(), 7);
	}

	#[test]
	fn link_toggle_adds_then_removes() {
		let mut app = App::new();
		let id1 = nid(1); // proc:2
		let id2 = nid(2); // filt:3 — no edge id1->id2 exists in the seed

		// add id1 -> id2
		app.connect_from = Some(id1);
		app.do_link_target(id2);
		assert_eq!(app.graph.connections().len(), 8);
		assert!(
			app.graph
				.connections()
				.iter()
				.any(|c| c.from_node() == id1 && c.to_node() == id2)
		);

		// toggle it off
		app.do_link_target(id2);
		assert_eq!(app.graph.connections().len(), 7);
		assert!(
			!app.graph
				.connections()
				.iter()
				.any(|c| c.from_node() == id1 && c.to_node() == id2)
		);
	}

	#[test]
	fn cycle_guard_refuses_back_edge() {
		let mut app = App::new();
		let id0 = nid(0); // src
		let id1 = nid(1); // proc:2 — seed has id0 -> id1

		// id1 -> id0 would close id0 -> id1 -> id0; must be refused.
		app.connect_from = Some(id1);
		app.do_link_target(id0);
		assert_eq!(app.graph.connections().len(), 7);
		assert!(
			!app.graph
				.connections()
				.iter()
				.any(|c| c.from_node() == id1 && c.to_node() == id0)
		);
	}

	#[test]
	fn delete_node_cascades_connections() {
		let mut app = App::new();
		// node #3 (proc:4) is touched by several edges; removing it must drop them.
		let before = app.graph.connections().len();
		app.state.selection = Some(nid(3));
		app.do_delete();
		assert!(!app.graph.has_node(nid(3)));
		assert!(app.graph.connections().len() < before);
		assert!(
			app.graph
				.connections()
				.iter()
				.all(|c| c.from_node() != nid(3) && c.to_node() != nid(3))
		);
	}

	#[test]
	fn selection_highlight_paints_border_in_theme_accent() {
		let mut app = App::new();
		// The selection highlight is the active theme's accent color.
		let accent = app.pal().accent.color();
		// area == canvas: every node is on-screen at offset (0,0).
		let area = Rect::new(0, 0, CANVAS_W, CANVAS_H);
		let target = nid(0); // src

		app.state = FlowState::new().select(Some(target));
		let inner = app
			.graph
			.split_stateful(area, &app.state)
			.into_iter()
			.find(|(id, _)| *id == target)
			.map(|(_, r)| r)
			.expect("selected node on-screen");
		assert!(inner.width > 0 && inner.height > 0);

		let mut buf = Buffer::empty(area);
		let mut st = app.state.clone();
		app.graph.clone().render(area, &mut buf, &mut st);

		// the node's real on-screen border = content rect expanded by 1 cell.
		let border = Rect {
			x: inner.x - 1,
			y: inner.y - 1,
			width: inner.width + 2,
			height: inner.height + 2,
		};
		let on_border = |x: u16, y: u16| {
			(x == border.x || x == border.right() - 1)
				&& (y >= border.y && y < border.bottom())
				|| (y == border.y || y == border.bottom() - 1)
					&& (x >= border.x && x < border.right())
		};
		let accent_on_border = (0..area.height)
			.flat_map(|y| (0..area.width).map(move |x| (x, y)))
			.filter(|(x, y)| {
				buf.cell(Position::new(*x, *y))
					.map(|c| c.style().fg == Some(accent))
					.unwrap_or(false)
			})
			.filter(|(x, y)| on_border(*x, *y))
			.count();
		assert!(
			accent_on_border > 0,
			"no accent cells on the selected node's border (got 0); \
			 highlight is mis-positioned"
		);
	}
}
