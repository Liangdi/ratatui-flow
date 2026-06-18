//! Machine-learning / data pipeline as a sci-fi themed flow graph (Nebula).
//!
//! Raw data is ingested and validated, then cleaned and enriched in parallel,
//! both feeding a feature store that trains a model, which is evaluated before
//! deployment.
//!
//! ```text
//! ingest ──▶ validate ──┬─▶ clean ──────┐
//!                        └─▶ enrich ─────┴─▶ features ──▶ train ──▶ eval ──▶ deploy
//! ```

use ratatui::style::Style;
use ratatui::widgets::BorderType;
use ratatui_flow::{Connection, FlowDirection, NodeGraph, NodeLayout};
use ratatui_sci_fi::{Rgb, Theme};

#[path = "support/harness.rs"]
mod harness;

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

const TITLES: [&str; 8] =
	["ingest", "validate", "clean", "enrich", "features", "train", "eval", "deploy"];
const BODIES: [&str; 8] = [
	"read s3\nparquet",
	"schema\nnull check",
	"dedupe\ntrim",
	"join dims\nencode",
	"feature\nstore",
	"fit model\n10 epochs",
	"metrics\nAUC / loss",
	"serve\nv2 canary",
];

fn main() -> std::io::Result<()> {
	let pal = Theme::Nebula.palette();
	let colors: [Rgb; 8] = [
		pal.accent,  // ingest
		pal.warn,    // validate
		pal.ok,      // clean
		pal.ok,      // enrich
		pal.accent2, // features
		pal.accent,  // train
		pal.warn,    // eval
		pal.alert,   // deploy
	];
	let nodes: Vec<NodeLayout<'static>> =
		(0..8).map(|i| node(TITLES[i], BODIES[i], colors[i].color())).collect();

	// ingest->validate ; validate->{clean,enrich} ; {clean,enrich}->features ;
	// features->train->eval->deploy
	let conns = vec![
		Connection::new(0usize.into(), 0usize.into(), 1usize.into(), 0usize.into()),
		Connection::new(1usize.into(), 0usize.into(), 2usize.into(), 0usize.into()),
		Connection::new(1usize.into(), 0usize.into(), 3usize.into(), 0usize.into()),
		Connection::new(2usize.into(), 0usize.into(), 4usize.into(), 0usize.into()),
		// enrich -> features enters on port 1 (distinct from clean's port 0).
		Connection::new(3usize.into(), 0usize.into(), 4usize.into(), 1usize.into()),
		Connection::new(4usize.into(), 0usize.into(), 5usize.into(), 0usize.into()),
		Connection::new(5usize.into(), 0usize.into(), 6usize.into(), 0usize.into()),
		Connection::new(6usize.into(), 0usize.into(), 7usize.into(), 0usize.into()),
	];

	let (w, h) = crossterm::terminal::size()?;
	let mut graph = NodeGraph::new(nodes, conns, w as usize, h as usize)
		.with_direction(FlowDirection::Rtl);
	graph.calculate();

	let bg = pal.bg.color();
	harness::show(|f| harness::render_flow(f, &graph, &BODIES, bg))
}
