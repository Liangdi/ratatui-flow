#[macro_use] extern crate log;
use std::collections::HashMap as Map;
use std::collections::BTreeSet as Set;
use tui::style::Color;
use tui::{
	layout::Rect,
	buffer::Buffer,
	style::Style,
	widgets::{Block, Widget, BorderType, Borders},
};

pub struct NodeLayout<'a> {
	// minimum size of contents (TODO: doc: including borders?)
	size: (u16, u16),
	border: BorderType,
	title: &'a str,
//	in_ports: Vec<PortLayout>,
//	out_ports: Vec<PortLayout>,
}

impl<'a> NodeLayout<'a> {
	pub fn new(size: (u16, u16)) -> Self {
		Self {
			size,
			border: BorderType::Rounded,
			title: "",
		}
	}

	pub fn with_title(mut self, title: &'a str) -> Self {
		self.title = title;
		self
	}
}

/*
pub struct PortLayout {
}
*/

#[derive(Default)]
pub struct NodeGraph<'a> {
	nodes: Vec<NodeLayout<'a>>,
	connections: Vec<Connection>,
	placements: Map<usize, Rect>,
	conn_layout: Map<Connection, ConnectionLayout>,
}

impl<'a> NodeGraph<'a> {
	pub fn new(
		nodes: Vec<NodeLayout<'a>>,
		connections: Vec<Connection>,
	) -> Self {
		Self {
			nodes,
			connections,
			..Self::default()
		}
	}

	pub fn calculate(&mut self) {
		self.placements.clear();

		// find root nodes
		let mut roots: Set<_> = (0..self.nodes.len()).collect();
		for ea_connection in self.connections.iter() {
			roots.remove(&ea_connection.from_node);
		}

		// place them and their children (recursively)
		let mut main_chain = Vec::new();
		for ea_root in roots {
			self.place_node(ea_root, 0, 0, &mut main_chain);
			assert!(main_chain.is_empty());
		}

		// calculate connections (eventually, this should be done during node
		// placement, but thats really complicated and i dont wanna deal with that
		// right now. essentially, adding non-trivial connections nudges nodes,
		// and nudging nodes nudges existing connections.)
		self.connections.sort_by(|a,b| {
			match a.from_node.cmp(&b.from_node) {
				std::cmp::Ordering::Equal => a.from_port.cmp(&b.from_node),
				other => other,
			}
		});
		let mut placed_ou_conns = Map::new();
		let mut placed_in_conns = Map::new();
		for ea_conn in self.connections.iter().copied() {
			let a_pos = self.placements[&ea_conn.from_node];
			let b_pos = self.placements[&ea_conn.to_node];
			let a_next_row = *placed_ou_conns.entry(ea_conn.from_node).or_insert(1);
			let b_next_row = *placed_in_conns.entry(ea_conn.to_node).or_insert(1);
			// NOTE: don't forget that left and right are swapped
			let a_point = (a_pos.left(), a_pos.top() + a_next_row);
			let b_point = (b_pos.right() + 1, b_pos.top() + b_next_row);
			// check for intersections
			let bad = 'isect: {
				let conn_bbox = Rect {
					x: a_point.0.min(b_point.0),
					y: a_point.1.min(b_point.1),
					width: a_point.0.abs_diff(b_point.0),
					height: a_point.1.abs_diff(b_point.1),
				};
				for ea_node in self.placements.values() {
					if conn_bbox.intersects(*ea_node) {
						// TODO: do something else when it intersects
						break 'isect true
					}
				}
				false
			};
			let layout = self.conn_layout.entry(ea_conn)
				.or_insert(ConnectionLayout::new(a_point, bad));
			if a_point.0 < b_point.0 {
				debug!("skipped {ea_conn:?}");
				continue
			}
			if a_point.1 != b_point.1 { // different heights
				let midpoint = (a_point.0 + b_point.0)/2;
				if midpoint != a_point.0 {
					// right
					layout.points.push((Direction::East, a_point.0 - midpoint));
				}
				// up/down
				if a_point.1 > b_point.1 {
					layout.points.push((Direction::North, a_point.1 - b_point.1));
				}
				else {
					layout.points.push((Direction::South, b_point.1 - a_point.1));
				}
				if midpoint != b_point.0 {
					// right
					layout.points.push((Direction::East, midpoint - b_point.0));
				}
			}
			else if a_point.0 != b_point.0 {
				layout.points.push((Direction::East, a_point.0 - b_point.0));
			}
			// `layout.points` should be empty if `a_point` and `b_point` are the same
			*placed_ou_conns.get_mut(&ea_conn.from_node).unwrap() += 1;
			*placed_in_conns.get_mut(&ea_conn.to_node).unwrap() += 1;
		}
	}

	/// ATTENTION: x_offs works in the opposite direction (higher values are
	/// further left) and y_offs is the same as tui (higher values are further
	/// down)
	fn place_node(&mut self, idx_node: usize, x: u16, y: u16, main_chain: &mut Vec<usize>) {
		// place the node
		let size_me = self.nodes[idx_node].size;
		let mut rect_me = Rect { x, y, width: size_me.0, height: size_me.1 };

		// nudge placement. if a node intersects with another node, its entire
		// main chain (largest subset of nodes including this one where every
		// node is the first child of its parent) should be moved down to not
		// intersect.
		let mut bottom = y;
		for (_, ea_them) in self.placements.iter() {
			if rect_me.intersects(*ea_them) {
				bottom = bottom.max(ea_them.bottom());
			}
		}
		for ea_node in main_chain.iter() {
			let y = self.placements[ea_node].y.max(bottom);
			self.placements.get_mut(ea_node).unwrap().y = y;
		}
		rect_me.y = bottom;
		self.placements.insert(idx_node, rect_me);

		// find children and order them
		let mut y = y;
		main_chain.push(idx_node);
		for ea_child in get_upstream(&self.connections, idx_node) {
			if self.placements.contains_key(&ea_child) {
				// nudge it (if necessary)
				self.nudge(ea_child, rect_me.x + rect_me.width + 3);
			}
			else {
				// place it
				self.place_node(ea_child, x + rect_me.width + 3, y, main_chain);
				main_chain.clear();
				y += self.placements[&ea_child].height;
			}
		}
		main_chain.pop();
	}

	fn nudge(&mut self, idx_node: usize, x: u16) {
		let rect_me = self.placements[&idx_node];
		if rect_me.x < x {
			self.placements.get_mut(&idx_node).unwrap().x = x;
			for ea_child in get_upstream(&self.connections, idx_node) {
				assert!(self.placements.contains_key(&ea_child));
				self.nudge(ea_child, x + rect_me.width + 3);
			}
		}
	}

	pub fn split(&self, area: Rect) -> Vec<Rect> {
		(0..self.nodes.len()).map(|idx_node| {
			self.placements.get(&idx_node).map(|pos| {
				if pos.right() > area.width || pos.bottom() > area.height {
					return Rect { x: 0, y: 0, width: 0, height: 0, }
				}
				let mut pos = *pos;
				pos.x = area.width - pos.right() + 1;
				pos.y += 1;
				pos.width -= 2;
				pos.height -= 2;
				pos
			})
			.unwrap_or_default()
		}).collect()
	}
}

fn get_upstream(conns: &Vec<Connection>, idx_node: usize) -> Vec<usize> {
	// find children and order them
	let mut upstream: Vec<_> = conns.iter()
		.filter(|ea| { ea.to_node == idx_node })
		.copied()
		.collect()
	;
	upstream.sort_by(|a,b| a.to_port.cmp(&b.to_port));
	upstream.iter().map(|ea| ea.from_node).collect()
}

fn get_downstream(conns: &Vec<Connection>, idx_node: usize) -> Vec<usize> {
	// find parents and order them
	let mut downstream: Vec<_> = conns.iter()
		.filter(|ea| { ea.from_node == idx_node })
		.copied()
		.collect()
	;
	downstream.sort_by(|a,b| a.from_port.cmp(&b.from_port));
	downstream.iter().map(|ea| ea.to_node).collect()
}


/*
pub struct NodeGraphState {
	x_offset: usize,
	y_offset: usize,
}
*/

#[derive(Debug, Clone)]
pub struct ConnectionLayout {
	start_pos: (u16, u16),
	points: Vec<(Direction, u16)>,
	border: BorderType,
	style: Style,
}

impl ConnectionLayout {
	fn new(start_pos: (u16, u16), bad: bool) -> Self {
		Self {
			start_pos,
			points: Vec::new(),
			border: BorderType::Double,
			style: if bad { Style::default().fg(Color::Red) }
				else { Style::default() },
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Direction {
	North,
	South,
	East,
	#[allow(unused)]
	West,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Connection {
	pub from_node: usize,
	pub from_port: usize,
	pub to_node: usize,
	pub to_port: usize,
}

impl Connection {
	pub fn new(from_node: usize, from_port: usize, to_node: usize, to_port: usize) -> Self {
		Self { from_node, from_port, to_node, to_port, }
	}
}

impl<'a> tui::widgets::StatefulWidget for NodeGraph<'a> {
	// eventually, this will contain stuff like view position
//	type State = NodeGraphState;
	type State = ();

	fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
		'node: for (idx_node, ea_node) in self.nodes.into_iter().enumerate() {
			if let Some(mut pos) = self.placements.get(&idx_node).copied() {
				if pos.right() > area.width || pos.bottom() > area.height { continue 'node }
				// draw box
				pos.x = area.width - pos.right();
				let block = Block::default().border_type(ea_node.border).borders(Borders::ALL).title(ea_node.title);
				block.render(pos, buf);
				// draw connection ports
				let mut row = 1;
				for _ea_conn in get_upstream(&self.connections, idx_node) {
					buf.get_mut(pos.left(), pos.top() + row)
						.set_symbol(conn_symbol(true, ea_node.border, BorderType::Double));
					row += 1;
				}
				let mut row = 1;
				for _ea_conn in get_downstream(&self.connections, idx_node) {
					buf.get_mut(pos.right() - 1, pos.top() + row)
						.set_symbol(conn_symbol(false, ea_node.border, BorderType::Double));
					row += 1;
				}
			}
			else {
				buf.set_string(0, idx_node as u16, format!("{idx_node}"), Style::default());
			}
		}

		// draw connections
		'conn: for ea_conn in self.connections.iter() {
			let layout = &self.conn_layout[&ea_conn];
			let symbols = BorderType::line_symbols(layout.border);
			let mut current_position = layout.start_pos;
			let mut last_dir = Direction::East;
			use Direction::*;
			for (ea_direction, ea_distance) in layout.points.iter() {
				let corner = match (ea_direction, last_dir) {
					(East,  East ) | (West,  West ) => symbols.horizontal,
					(East,  West ) | (West,  East ) => unreachable!(),
					(North, North) | (South, South) => symbols.vertical,
					(North, South) | (South, North) => unreachable!(),
					(East,  North) | (South, West ) => symbols.top_left,
					(West,  North) | (South, East ) => symbols.top_right,
					(East,  South) | (North, West ) => symbols.bottom_left,
					(West,  South) | (North, East ) => symbols.bottom_right,
				};
				if current_position.0 > area.width || current_position.1 >= area.bottom() { continue 'conn }
				buf.get_mut(area.width - current_position.0, current_position.1)
					.set_symbol(corner)
					.set_style(layout.style);
				match ea_direction {
					Direction::East => {
						for idx in 1..*ea_distance {
							if current_position.0 - idx > area.width || current_position.1 >= area.bottom() { continue 'conn }
							buf.get_mut(area.width - (current_position.0 - idx), current_position.1)
								.set_symbol(symbols.horizontal)
								.set_style(layout.style);
						}
						current_position.0 -= ea_distance;
					}
					Direction::West => {
						for idx in 1..*ea_distance {
							if current_position.0 + idx > area.width || current_position.1 >= area.bottom() { continue 'conn }
							buf.get_mut(area.width - (current_position.0 + idx), current_position.1)
								.set_symbol(symbols.horizontal)
								.set_style(layout.style);
						}
						current_position.0 += ea_distance;
					}
					Direction::North => {
						for idx in 1..*ea_distance {
							if current_position.0 > area.width || current_position.1 - idx >= area.bottom() { continue 'conn }
							buf.get_mut(area.width - current_position.0, current_position.1 - idx)
								.set_symbol(symbols.vertical)
								.set_style(layout.style);
						}
						current_position.1 -= ea_distance;
					}
					Direction::South => {
						for idx in 1..*ea_distance {
							if current_position.0 > area.width || current_position.1 + idx >= area.bottom() { continue 'conn }
							buf.get_mut(area.width - current_position.0, current_position.1 + idx)
								.set_symbol(symbols.vertical)
								.set_style(layout.style);
						}
		            current_position.1 += ea_distance;
					}
				}
				last_dir = *ea_direction;
			}
			if current_position.0 > area.width || current_position.1 >= area.bottom() { continue 'conn }
			let corner = match last_dir {
				East  => symbols.horizontal,
				West  => unreachable!(),
				North => symbols.top_left,
				South => symbols.bottom_left,
			};
			buf.get_mut(area.width - current_position.0, current_position.1)
				.set_symbol(corner)
				.set_style(layout.style);
		}
	}
}

fn conn_symbol(direction: bool, block_style: BorderType, conn_style: BorderType) -> &'static str {
	let out = match (block_style, conn_style) {
		(BorderType::Plain
		|BorderType::Rounded, BorderType::Thick)   => ("┥", "┝"),
		(BorderType::Plain
		|BorderType::Rounded, BorderType::Double)  => ("╡", "╞"),
		(BorderType::Plain
		|BorderType::Rounded, BorderType::Plain
		                    | BorderType::Rounded) => ("┤", "├"),

		(BorderType::Thick,   BorderType::Thick)   => ("┫", "┣"),
		(BorderType::Thick,   BorderType::Double)  => ("X", "X"),
		(BorderType::Thick,   BorderType::Plain
		                    | BorderType::Rounded) => ("┨", "┠"),

		(BorderType::Double,  BorderType::Thick)   => ("X", "X"),
		(BorderType::Double,  BorderType::Double)  => ("╣", "╠"),
		(BorderType::Double,  BorderType::Plain
		                    | BorderType::Rounded) => ("╢", "╟"),
	};
	if direction { out.0 } else { out.1 }
}
