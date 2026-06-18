//! CI/CD pipeline as a sci-fi themed flow graph (DeepSpace).
//!
//! A classic release flow: a commit triggers a build, which fans out to tests
//! and lint, both gate a package step, which deploys to staging then prod.
//!
//! ```text
//! commit ──▶ build ──┬─▶ test ──┐
//!                    └─▶ lint ──┴─▶ package ──▶ staging ──▶ prod
//! ```

use ratatui::style::Style;
use ratatui::widgets::BorderType;
use ratatui_flow::{Connection, FlowDirection, NodeGraph, NodeLayout};
use ratatui_sci_fi::{Rgb, Theme};

#[path = "support/harness.rs"]
mod harness;

/// Themed node: a titled rounded box whose border color is `fg`.
fn node(
	title: &'static str,
	body: &'static str,
	fg: ratatui::style::Color,
) -> NodeLayout<'static> {
	NodeLayout::from_content(body)
		.with_title(title)
		.with_border_type(BorderType::Rounded)
		.with_border_style(Style::default().fg(fg))
}

const TITLES: [&str; 7] = ["git", "build", "test", "lint", "package", "staging", "prod"];
const BODIES: [&str; 7] = [
	"commit\npush",
	"cargo build\n--release",
	"cargo test",
	"clippy\nfmt --check",
	"docker build\ntag :latest",
	"deploy\nstaging",
	"deploy\nproduction",
];

fn main() -> std::io::Result<()> {
	let pal = Theme::DeepSpace.palette();
	// Each node's border maps to a semantic palette slot.
	let colors: [Rgb; 7] = [
		pal.accent,  // git
		pal.accent2, // build
		pal.ok,      // test
		pal.warn,    // lint
		pal.accent,  // package
		pal.accent2, // staging
		pal.alert,   // prod
	];
	let nodes: Vec<NodeLayout<'static>> =
		(0..7).map(|i| node(TITLES[i], BODIES[i], colors[i].color())).collect();

	// git->build ; build->{test,lint} ; {test,lint}->package ; package->staging->prod
	let conns = vec![
		Connection::new(0usize.into(), 0usize.into(), 1usize.into(), 0usize.into()),
		Connection::new(1usize.into(), 0usize.into(), 2usize.into(), 0usize.into()),
		Connection::new(1usize.into(), 0usize.into(), 3usize.into(), 0usize.into()),
		Connection::new(2usize.into(), 0usize.into(), 4usize.into(), 0usize.into()),
		// lint -> package enters on port 1 (distinct from test's port 0) so the
		// two converging connections route cleanly instead of aliasing.
		Connection::new(3usize.into(), 0usize.into(), 4usize.into(), 1usize.into()),
		Connection::new(4usize.into(), 0usize.into(), 5usize.into(), 0usize.into()),
		Connection::new(5usize.into(), 0usize.into(), 6usize.into(), 0usize.into()),
	];

	let (w, h) = crossterm::terminal::size()?;
	let mut graph = NodeGraph::new(nodes, conns, w as usize, h as usize)
		.with_direction(FlowDirection::Rtl);
	graph.calculate();

	let bg = pal.bg.color();
	harness::show(|f| harness::render_flow(f, &graph, &BODIES, bg))
}
