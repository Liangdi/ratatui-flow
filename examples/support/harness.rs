//! Shared terminal harness for the themed flow-graph examples.
//!
//! NOT a standalone example (no `main`); each example pulls it in via
//! `#[path = "support/harness.rs"] mod harness;`. It sets up the terminal,
//! draws a single frame through a closure, holds until any key, and restores —
//! so the example file itself stays focused on its graph topology.

use crossterm::event;
use crossterm::execute;
use crossterm::terminal::{
	EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::{
	Frame, Terminal,
	backend::CrosstermBackend,
	style::{Color, Style},
	widgets::Paragraph,
};
use ratatui_flow::{FlowState, NodeGraph};
use std::io;

/// Enable the terminal, draw one frame, block until any key, then restore.
pub fn show(draw: impl FnOnce(&mut Frame)) -> io::Result<()> {
	enable_raw_mode()?;
	let mut out = io::stdout();
	execute!(out, EnterAlternateScreen)?;
	struct Guard;
	impl Drop for Guard {
		fn drop(&mut self) {
			let _ = disable_raw_mode();
			let _ = execute!(io::stdout(), LeaveAlternateScreen);
		}
	}
	let _guard = Guard;
	let mut terminal = Terminal::new(CrosstermBackend::new(out))?;
	terminal.draw(draw)?;
	let _ = event::read();
	Ok(())
}

/// Render a flow graph full-screen on a themed background: node bodies into
/// their interior rects (from `split`), then borders/ports/connections on top.
/// `bodies[i]` is the content of the i-th node (insertion order).
pub fn render_flow(f: &mut Frame, graph: &NodeGraph<'_>, bodies: &[&str], bg: Color) {
	let area = f.area();
	f.buffer_mut().set_style(area, Style::default().bg(bg));
	for (i, z) in graph.split(area).iter().enumerate() {
		if z.width > 0
			&& z.height > 0
			&& let Some(body) = bodies.get(i)
		{
			f.render_widget(Paragraph::new(*body), *z);
		}
	}
	let mut state = FlowState::default();
	f.render_stateful_widget(graph.clone(), area, &mut state);
}
