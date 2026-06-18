//! Microservice request flow as a sci-fi themed flow graph (Cyberpunk).
//!
//! A client request enters an API gateway, which fans out through auth and
//! rate-limiting to three services (orders / payments / inventory) that converge
//! on shared storage (db / cache).
//!
//! ```text
//!                              ┌─▶ orders    ─┐
//! client ──▶ gateway ──┬─▶ auth ─┼─▶ payments ─┼─▶ db
//!                      └─▶ limit ─▶ inventory ──▶ cache
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
	["client", "gateway", "auth", "limit", "orders", "payments", "db", "cache"];
const BODIES: [&str; 8] = [
	"GET /api",
	"route\nTLS",
	"jwt\nverify",
	"100 rps\nper ip",
	"orders\nsvc",
	"payments\nsvc",
	"postgres\nprimary",
	"redis\nhot keys",
];

fn main() -> std::io::Result<()> {
	let pal = Theme::Cyberpunk.palette();
	let colors: [Rgb; 8] = [
		pal.accent,  // client
		pal.accent2, // gateway
		pal.warn,    // auth
		pal.warn,    // limit
		pal.ok,      // orders
		pal.ok,      // payments
		pal.fg,      // db
		pal.muted,   // cache
	];
	let nodes: Vec<NodeLayout<'static>> =
		(0..8).map(|i| node(TITLES[i], BODIES[i], colors[i].color())).collect();

	// client->gateway ; gateway->{auth,limit} ; auth->{orders,payments} ;
	// limit->payments ; {orders,payments}->db ; payments->cache
	let conns = vec![
		Connection::new(0usize.into(), 0usize.into(), 1usize.into(), 0usize.into()),
		Connection::new(1usize.into(), 0usize.into(), 2usize.into(), 0usize.into()),
		Connection::new(1usize.into(), 0usize.into(), 3usize.into(), 0usize.into()),
		Connection::new(2usize.into(), 0usize.into(), 4usize.into(), 0usize.into()),
		// auth -> payments enters on port 1 (distinct from limit's port 0).
		Connection::new(2usize.into(), 0usize.into(), 5usize.into(), 1usize.into()),
		Connection::new(3usize.into(), 0usize.into(), 5usize.into(), 0usize.into()),
		Connection::new(4usize.into(), 0usize.into(), 6usize.into(), 0usize.into()),
		// payments -> db enters on port 1 (distinct from orders' port 0).
		Connection::new(5usize.into(), 0usize.into(), 6usize.into(), 1usize.into()),
		Connection::new(5usize.into(), 0usize.into(), 7usize.into(), 0usize.into()),
	];

	let (w, h) = crossterm::terminal::size()?;
	let mut graph = NodeGraph::new(nodes, conns, w as usize, h as usize)
		.with_direction(FlowDirection::Rtl);
	graph.calculate();

	let bg = pal.bg.color();
	harness::show(|f| harness::render_flow(f, &graph, &BODIES, bg))
}
