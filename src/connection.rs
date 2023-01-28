use std::collections::BTreeMap as Map;
use tui::{
	style::Style,
	widgets::BorderType, buffer::Buffer, layout::Rect,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
	North = 0,
	South = 1,
	East = 2,
	West = 3,
}

impl Direction {
	fn is_vertical(&self) -> bool {
		(*self as usize) < 2
	}
	/*
	fn invert(self) -> Self {
		use Direction as D;
		match self {
			D::North => D::South,
			D::South => D::North,
			D::East => D::West,
			D::West => D::East,
		}
	}
	fn rotate(self) -> Self {
		use Direction as D;
		match self {
			D::North => D::East,
			D::East => D::South,
			D::South => D::West,
			D::West => D::North,
		}
	}
	*/
}

impl std::fmt::Debug for Direction {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let print = match self {
			Direction::North => '↑',
			Direction::South => '↓',
			Direction::East => '→',
			Direction::West => '←',
		};
		write!(f, "{}", print)
	}
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

#[derive(Debug, Clone)]
pub struct ConnectionLayout {
	start_pos: (u16, u16),
	points: Vec<(Direction, u16)>,
	border: BorderType,
	style: Style,
}

impl ConnectionLayout {
	pub fn new(start_pos: (u16, u16)) -> Self {
		Self {
			start_pos,
			points: Vec::new(),
			border: BorderType::Rounded,
			style: Style::default(),
		}
	}

	pub fn start_pos(&self) -> (u16, u16) {
		self.start_pos
	}
	pub fn style(&self) -> Style {
		self.style
	}
	pub fn border(&self) -> BorderType {
		self.border
	}
	pub fn push(&mut self, item: (Direction, u16)) {
		self.points.push(item)
	}
	pub fn points(&self) -> &Vec<(Direction, u16)> {
		&self.points
	}
}

/// Generate the correct connection symbol for this node
pub fn conn_symbol(is_input: bool, block_style: BorderType, conn_style: BorderType) -> &'static str {
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
	if is_input { out.0 } else { out.1 }
}

pub const ALIAS_CHARS: [&str; 24] = [
	"α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ", "ν", "ξ", "ο", "π", "ρ", "σ", "τ", "υ", "φ", "χ", "ψ", "ω",
];

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
	#[default]
	Empty,
	Blocked,
	Connection(usize),
}
const E: Edge = Edge::Empty;

#[derive(Debug)]
pub struct ConnectionsLayout {
	ports: Map<(bool, usize, usize), (usize, usize)>, // (x,y)
	connections: Vec<(Connection, usize)>, // ((from, to), class)
	edge_field: Betweens<Edge>,
	width: usize,
	height: usize,
}

impl ConnectionsLayout {
	pub fn new(width: usize, height: usize) -> Self {
		Self {
			ports: Map::new(),
			connections: Vec::new(),
			edge_field: Betweens::new(width, height),
			width,
			height,
		}
	}

	pub fn push_connection(&mut self, connection: (Connection, usize)) {
		self.connections.push(connection)
	}

	pub fn insert_port(&mut self, is_input: bool, node: usize, port: usize, pos: (usize, usize)) {
		self.ports.insert((is_input, node, port), pos);
	}

	pub fn calculate(&mut self) {
		'outer: for ea_conn in &self.connections {
			let start = (self.ports[&(false, ea_conn.0.from_node, ea_conn.0.from_port)], Direction::East);
			let goal  = (self.ports[&(true, ea_conn.0.to_node, ea_conn.0.to_port)],      Direction::West);
			if start.0.0 > self.edge_field.width || start.0.1 > self.edge_field.height {
				continue
			}
			if goal.0.0 > self.edge_field.width || goal.0.1 > self.edge_field.height {
				continue
			}
			//println!("drawing connection {start:?} to {goal:?}");
			let mut frontier = sorted_vec::SortedVec::new();
			let mut came_from = Betweens::<Option<_>>::new(self.width, self.height);
			let mut cost      = Betweens::<isize>::new(self.width, self.height);
			frontier.push(((0, 0), start));
			while let Some((_, current)) = frontier.pop() {
				if current == goal {
					break
				}
				for ea_nei in neighbors(current.0, self.width, self.height) {
					let ea_edge = ea_nei.into();
					let current_cost = cost[current.into()];
			//		println!("{current_cost}");
					let new_cost = current_cost.saturating_add(self.calc_cost(current, ea_nei, start.0, goal.0, ea_conn.1));
					if came_from[ea_edge].is_none() || new_cost < cost[ea_edge] {
						let prio = (-new_cost, -Self::heuristic(ea_nei.0, goal.0));
						if new_cost != isize::MAX {
							frontier.push((prio, ea_nei));
						}
						came_from[ea_edge] = Some(current);
						cost[ea_edge] = new_cost;
					}
				}
				/*
				print!("\x1b[2J\x1b[1;1H");
				println!("{frontier:?}");
				let mut prio = Betweens::new(self.width, self.height);
				for ea_front in frontier.iter() {
					prio[ea_front.1.into()] = ea_front.0;
				}
				println!("prio\n");
				prio.print_with(4, |ea| print!("{:>4} ", ea.0));
				prio.print_with(4, |ea| print!("{:>4} ", ea.1));
				println!("cost\n");
				cost.print_with(4, |ea| print!("{:>4} ", ea));
				println!("from\n");
				came_from.print_with(1, |ea| {
					if let Some(inner) = ea {
						print!("{:?} ", inner.1);
					}
					else {
						print!("_ ");
					}
				});
				std::io::stdin().read_line(&mut String::new()).unwrap();
				*/
			}
			// TODO: mark connections that didnt reach the goal
			let mut next = goal;
			loop {
				if next == start { break }
				self.edge_field[next.into()] = Edge::Connection(ea_conn.1);
				if let Some(from) = came_from[next.into()] {
					next = from;
				}
				else {
					println!("couldnt connect {start:?} to {goal:?}");
					continue 'outer
				}
			}
		}
	}

	pub fn render(&self, area: Rect, buf: &mut Buffer) {
		let sym = BorderType::line_symbols(BorderType::Plain);
		let dub = BorderType::line_symbols(BorderType::Rounded);

		for ea_conn in self.connections.iter() { println!("{ea_conn:?}"); }
		for ea_port in self.ports.iter() { println!("{ea_port:?}"); }
	//	self.edge_field.print_with(1, |ea| print!("{:>1} ", ea));
	//	println!("{}{}{}", dub.top_left, dub.horizontal.repeat(self.width), dub.top_right);
		for y in 0..self.height {
	//		print!("{}", dub.vertical);
			for x in 0..self.width {
				let pos = (x,y);
				let north = self.edge_field[(pos, Direction::North).into()];
				let south = self.edge_field[(pos, Direction::South).into()];
				let east  = self.edge_field[(pos, Direction::East).into()];
				let west  = self.edge_field[(pos, Direction::West).into()];
				let symbol = match (north, south, east, west) {
					(E, E, E, E) => " ",
					(n, s, e, w) if n == s && n == e && n == w => sym.cross,
					(n, s, e, w) if n == s && e == w && n != E && e != E => sym.vertical, // intersections should just be verticals
					(n, s, E, w) if n == s && n == w => sym.vertical_left,
					(n, E, e, w) if n == e && n == w => sym.horizontal_up,
					(n, s, e, E) if n == s && n == e => sym.vertical_right,
					(E, s, e, w) if s == e && s == w => sym.horizontal_down,
					(n, s, E, E) if n == s => sym.vertical,
					(E, E, e, w) if e == w => sym.horizontal,
					(E, s, E, w) if s == w => sym.top_right,
					(n, E, E, w) if n == w => sym.bottom_right,
					(n, E, e, E) if n == e => sym.bottom_left,
					(E, s, e, E) if s == e => sym.top_left,

					(_, E, E, E) => "↑",
					(E, _, E, E) => "↓",
					(E, E, _, E) => "→",
					(E, E, E, _) => "←",
					(n, s, e, w) => "?",//unreachable!("{n} {s} {e} {w}"),
				};
				buf.get_mut(x as u16, y as u16)
					.set_symbol(symbol)
				//	.set_style(ea_layout.style())
				;
			}
	//		println!("{}", dub.vertical);
		}

	//	println!("{}{}{}", dub.bottom_left, dub.horizontal.repeat(self.width), dub.bottom_right);
	}

	fn heuristic(from: (usize, usize), to: (usize, usize)) -> isize {
		(from.0 as isize - to.0 as isize).pow(2) + (from.1 as isize - to.1 as isize).pow(2)
	}

	fn calc_cost(&self, current: ((usize, usize), Direction), neigh: ((usize, usize), Direction), start: (usize, usize), end: (usize, usize), conn_t: usize) -> isize {
		let conn_t = Edge::Connection(conn_t);
		let north = self.edge_field[(current.0, Direction::North).into()];
		let south = self.edge_field[(current.0, Direction::South).into()];
		let east  = self.edge_field[(current.0, Direction::East).into()];
		let west  = self.edge_field[(current.0, Direction::West).into()];

		let in_dir = self.edge_field[current.into()];
		// TODO: fix
		if !(in_dir == Edge::Empty || in_dir == conn_t) {
			return isize::MAX
		}
	//	assert!(in_dir == 0 || in_dir == conn_t); // should only calculate cost if its possible
		let out_dir = self.edge_field[neigh.into()];
		if out_dir == conn_t {
			// already exists
			1
		}
		else if out_dir == Edge::Empty {
			if north == conn_t || south == conn_t || east == conn_t || west == conn_t {
				// intersecting with an existing connection
				2 // maybe multiply with distances?
			}
			else {
				let in_is_vert = current.1.is_vertical();
				let out_is_vert = neigh.1.is_vertical();
				let straight = in_is_vert == out_is_vert;
				if straight {
					if north == Edge::Empty && south == Edge::Empty && east == Edge::Empty && west == Edge::Empty {
						2
					}
					else {
						4
					}
				}
				else {
					// curved
					if north != Edge::Empty || south != Edge::Empty || east != Edge::Empty || west != Edge::Empty {
						isize::MAX
					}
					else {
						let ax = current.0.0 as isize;
						let ay = current.0.1 as isize;
						let sx = start.0 as isize;
						let sy = start.1 as isize;
						let ex = end.0 as isize;
						let ey = end.1 as isize;
						4 + (
							(ax-sx).pow(2) + (ay-sy).pow(2) +
							(ax-ex).pow(2) + (ay-ey).pow(2)
						)
					}
				}
			}
		}
		else {
			isize::MAX
		}
	}
}

fn neighbors(pos: (usize, usize), width: usize, height: usize) -> Vec<((usize, usize), Direction)> {
	let mut out = Vec::new();
	if pos.0 < width-1  { out.push(((pos.0 + 1, pos.1), Direction::West)); }
	if pos.1 < height-1 { out.push(((pos.0, pos.1 + 1), Direction::North)); }
	if pos.0 > 0        { out.push(((pos.0 - 1, pos.1), Direction::East)); }
	if pos.1 > 0        { out.push(((pos.0, pos.1 - 1), Direction::South)); }
	out
}

use core::ops::{Index, IndexMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeIdx(usize, usize, bool);
/*
impl EdgeIdx {
	fn pos(self) -> (usize, usize) {
		(self.0, self.1)
	}
}
*/
impl From<((usize, usize), Direction)> for EdgeIdx {
	fn from(value: ((usize, usize), Direction)) -> Self {
		match value.1 {
			Direction::North => Self(value.0.0,   value.0.1,   true),
			Direction::South => Self(value.0.0,   value.0.1+1, true),
			Direction::East  => Self(value.0.0+1, value.0.1,  false),
			Direction::West  => Self(value.0.0,   value.0.1,  false),
		}
	}
}

// the outermost values are unnecessary
#[derive(Debug)]
struct Betweens<T: Default> {
	horizontal: Vec<Vec<T>>,
	vertical: Vec<Vec<T>>,
	width: usize,
	height: usize,
}
impl<T: Default> Index<EdgeIdx> for Betweens<T> {
	type Output = T;
	fn index(&self, index: EdgeIdx) -> &Self::Output {
		if index.2 { &self.vertical[index.1][index.0] }
		else       { &self.horizontal[index.1][index.0] }
	}
}
impl<T: Default> IndexMut<EdgeIdx> for Betweens<T> {
	fn index_mut(&mut self, index: EdgeIdx) -> &mut T {
		if index.2 { &mut self.vertical[index.1][index.0] }
		else       { &mut self.horizontal[index.1][index.0] }
	}
}

impl<T: Default> Betweens<T> {
	fn new(x: usize, y: usize) -> Self {
		let mut out = Self {
			horizontal: Vec::new(),
			vertical: Vec::new(),
			width: 0,
			height: 0,
		};
		out.set_size(x, y);
		out
	}

	fn set_size(&mut self, x: usize, y: usize) {
		self.horizontal.resize_with(y, || {
			let mut inner = Vec::new();
			inner.resize_with(x + 1, Default::default);
			inner
		});
		self.vertical.resize_with(y + 1, || {
			let mut inner = Vec::new();
			inner.resize_with(x, Default::default);
			inner
		});
		self.width = x;
		self.height = y;
	}

	#[allow(unused)]
	fn print_with(&self, width: usize, f: impl Fn(&T)) {
		for y in 0..(self.height+1) {
			for x in 0..self.width {
				print!("{} ", "-".repeat(width));
				f(&self.vertical[y][x]);
			}
			println!("{}", "-".repeat(width));
			if y < self.height {
				for x in 0..(self.width+1) {
					f(&self.horizontal[y][x]);
					if x < self.width {
						print!("{} ", "-".repeat(width));
					}
				}
			}
			println!();
		}
	}
}
