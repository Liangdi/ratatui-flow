use ratatui::{
	buffer::Buffer,
	layout::{Position, Rect},
	style::{Color, Style},
	symbols::line,
	widgets::BorderType,
};
use std::collections::{BTreeMap as Map, BinaryHeap};

use crate::id::{NodeId, PortId};

const SEARCH_TIMEOUT: usize = 5000;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineType {
	#[default]
	Plain,
	Rounded,
	Double,
	Thick,
}

impl LineType {
	fn to_line_set(self) -> line::Set<'static> {
		match self {
			LineType::Plain => line::NORMAL,
			LineType::Rounded => line::ROUNDED,
			LineType::Double => line::DOUBLE,
			LineType::Thick => line::THICK,
		}
	}
}

impl From<BorderType> for LineType {
	fn from(value: BorderType) -> Self {
		match value {
			BorderType::Plain => LineType::Plain,
			BorderType::Rounded => LineType::Rounded,
			BorderType::Double => LineType::Double,
			BorderType::Thick => LineType::Thick,
			BorderType::LightDoubleDashed
			| BorderType::LightTripleDashed
			| BorderType::LightQuadrupleDashed => LineType::Plain,
			BorderType::HeavyDoubleDashed
			| BorderType::HeavyTripleDashed
			| BorderType::HeavyQuadrupleDashed => LineType::Thick,
			BorderType::QuadrantInside | BorderType::QuadrantOutside => LineType::Plain,
		}
	}
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Direction {
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

#[derive(Debug, Clone, Copy)]
pub struct Connection<'a> {
	pub(crate) from_node: NodeId,
	pub(crate) from_port: PortId,
	pub(crate) to_node: NodeId,
	pub(crate) to_port: PortId,
	line_type: LineType,
	line_style: Style,
	label: Option<&'a str>,
}

impl<'a> Connection<'a> {
	pub fn new(
		from_node: NodeId,
		from_port: PortId,
		to_node: NodeId,
		to_port: PortId,
	) -> Self {
		Self {
			from_node,
			from_port,
			to_node,
			to_port,
			line_type: LineType::Rounded,
			line_style: Style::default(),
			label: None,
		}
	}

	pub fn from_node(&self) -> NodeId {
		self.from_node
	}

	pub fn from_port(&self) -> PortId {
		self.from_port
	}

	pub fn to_node(&self) -> NodeId {
		self.to_node
	}

	pub fn to_port(&self) -> PortId {
		self.to_port
	}

	pub fn with_line_type(mut self, line_type: LineType) -> Self {
		self.line_type = line_type;
		self
	}

	pub fn line_type(&self) -> LineType {
		self.line_type
	}

	pub fn with_line_style(mut self, line_style: Style) -> Self {
		self.line_style = line_style;
		self
	}

	pub fn line_style(&self) -> Style {
		self.line_style
	}

	/// Attach a **label** to this connection, rendered horizontally at the
	/// midpoint of the routed path on top of the line (with a solid background
	/// so the line stays readable under the text).
	///
	/// Labels are purely opt-in: a connection built without `with_label` has
	/// `label == None` and renders identically to a label-less graph.
	///
	/// The label is borrowed for the connection's lifetime `'a`, so `Connection`
	/// stays `Copy` and can be used exactly as before (`add_connection`,
	/// `.copied()`, etc.). Passing a `&'static str` is the common case.
	///
	/// The label text is truncated on screen to fit (see [`label`]); in
	/// vertical layouts (`Ttb`/`Btt`) it is still written horizontally (the
	/// terminal cannot rotate text), which may overlap neighbouring nodes —
	/// this is an accepted visual compromise.
	///
	/// [`label`]: Self::label
	#[must_use]
	pub fn with_label(mut self, label: &'a str) -> Self {
		self.label = Some(label);
		self
	}

	/// The label attached via [`with_label`], or `None` (the default).
	pub fn label(&self) -> Option<&str> {
		self.label
	}
}

/// Layout/routing problems detected during [`NodeGraph::calculate`][crate::NodeGraph::calculate] that would
/// otherwise fail silently (a node not placed, a bad connection ignored, or a
/// connection that couldn't be routed and fell back to an alias character).
///
/// Retrieve them via [`NodeGraph::diagnostics`][crate::NodeGraph::diagnostics].
/// They are also emitted through `log::warn!`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
	/// A node is unreachable (not on any root node's upstream chain — e.g. a pure
	/// cycle or an isolated node) and was therefore not placed.
	UnplacedNode {
		node: NodeId,
	},
	/// A connection referenced an out-of-bounds node index and was ignored.
	InvalidConnectionRef {
		from_node: NodeId,
		to_node: NodeId,
	},
	/// A connection could not be routed (the search timed out or found no path)
	/// and was downgraded to an alias character for display.
	RoutingFailed {
		from_node: NodeId,
		from_port: PortId,
		to_node: NodeId,
		to_port: PortId,
	},
}

/// Generate the correct connection symbol for this node.
///
/// `is_input` selects the in-port (┤) vs out-port (├) variant. `vertical`
/// switches between horizontal-edge ports (drawn on the left/right border, the
/// original `Ltr`/`Rtl` behavior) and vertical-edge ports (drawn on the
/// top/bottom border for `Ttb`/`Btt`).
pub(crate) fn conn_symbol(
	is_input: bool,
	block_style: BorderType,
	conn_style: LineType,
	vertical: bool,
) -> &'static str {
	// Each entry is (input_symbol, output_symbol). For horizontal ports the
	// input is on the left edge (┤ family) and output on the right (├ family);
	// for vertical ports input is on the top edge (┴ family) and output on the
	// bottom (┬ family).
	let out = if !vertical {
		match (block_style, conn_style) {
			(BorderType::Plain | BorderType::Rounded, LineType::Thick) => ("┥", "┝"),
			(BorderType::Plain | BorderType::Rounded, LineType::Double) => ("╡", "╞"),
			(
				BorderType::Plain | BorderType::Rounded,
				LineType::Plain | LineType::Rounded,
			) => ("┤", "├"),

			(BorderType::Thick, LineType::Thick) => ("┫", "┣"),
			(BorderType::Thick, LineType::Double) => ("╡", "╞"), // fallback
			(BorderType::Thick, LineType::Plain | LineType::Rounded) => ("┨", "┠"),

			(BorderType::Double, LineType::Thick) => ("╢", "╟"), // fallback
			(BorderType::Double, LineType::Double) => ("╣", "╠"),
			(BorderType::Double, LineType::Plain | LineType::Rounded) => ("╢", "╟"),

			(
				BorderType::LightDoubleDashed
				| BorderType::LightTripleDashed
				| BorderType::LightQuadrupleDashed,
				_,
			) => ("┤", "├"),
			(
				BorderType::HeavyDoubleDashed
				| BorderType::HeavyTripleDashed
				| BorderType::HeavyQuadrupleDashed,
				_,
			) => ("┤", "├"),
			(BorderType::QuadrantInside | BorderType::QuadrantOutside, _) => ("┤", "├"),
		}
	} else {
		// vertical-edge ports: ┴ (input/top) and ┬ (output/bottom) families.
		match (block_style, conn_style) {
			(BorderType::Plain | BorderType::Rounded, LineType::Thick) => ("┻", "┳"),
			(BorderType::Plain | BorderType::Rounded, LineType::Double) => ("╨", "╥"),
			(
				BorderType::Plain | BorderType::Rounded,
				LineType::Plain | LineType::Rounded,
			) => ("┴", "┬"),

			(BorderType::Thick, LineType::Thick) => ("┻", "┳"),
			(BorderType::Thick, LineType::Double) => ("╨", "╥"), // fallback
			(BorderType::Thick, LineType::Plain | LineType::Rounded) => ("┴", "┬"),

			(BorderType::Double, LineType::Thick) => ("╨", "╥"), // fallback
			(BorderType::Double, LineType::Double) => ("╩", "╦"),
			(BorderType::Double, LineType::Plain | LineType::Rounded) => ("╨", "╥"),

			(
				BorderType::LightDoubleDashed
				| BorderType::LightTripleDashed
				| BorderType::LightQuadrupleDashed,
				_,
			) => ("┴", "┬"),
			(
				BorderType::HeavyDoubleDashed
				| BorderType::HeavyTripleDashed
				| BorderType::HeavyQuadrupleDashed,
				_,
			) => ("┴", "┬"),
			(BorderType::QuadrantInside | BorderType::QuadrantOutside, _) => ("┴", "┬"),
		}
	};
	if is_input { out.0 } else { out.1 }
}

/// The direction-arrow symbol drawn on a connection's `to` port (the in port,
/// where the line enters the node), pointing in the direction of flow — i.e.
/// *into* the `to` node, the way the data/contribution flows. The variant
/// scales with `line_type`: heavy line types (`Thick`/`Double`) use the solid
/// heavy arrows (`◀▶▼▲`); light types (`Plain`/`Rounded`) use the thin arrows
/// (`◄►▽△`).
///
/// This **replaces** the in-port connection glyph (`┤`/`┴` family) when
/// [`NodeGraph::show_arrows`][crate::NodeGraph] is enabled (the default), so
/// the flow direction is visible at a glance. The out port (`from_node` side)
/// keeps its `├`/`┬` family glyph regardless.
///
/// Direction → pointing:
/// - `Rtl` (flow right→left): points left `◄`/`◀`
/// - `Ltr` (flow left→right): points right `►`/`▶`
/// - `Ttb` (flow top→bottom): points down `▼`/`▽`
/// - `Btt` (flow bottom→top): points up `▲`/`△`
pub(crate) fn arrow_symbol(
	direction: crate::FlowDirection,
	line_type: LineType,
) -> &'static str {
	let heavy = matches!(line_type, LineType::Thick | LineType::Double);
	match direction {
		crate::FlowDirection::Rtl => {
			if heavy {
				"◀"
			} else {
				"◄"
			}
		}
		crate::FlowDirection::Ltr => {
			if heavy {
				"▶"
			} else {
				"►"
			}
		}
		crate::FlowDirection::Ttb => {
			if heavy {
				"▼"
			} else {
				"▽"
			}
		}
		crate::FlowDirection::Btt => {
			if heavy {
				"▲"
			} else {
				"△"
			}
		}
	}
}

pub(crate) const ALIAS_CHARS: [&str; 24] = [
	"α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ", "ν", "ξ", "ο", "π", "ρ",
	"σ", "τ", "υ", "φ", "χ", "ψ", "ω",
];

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
	#[default]
	Empty,
	Blocked,
	Connection(usize),
}
const E: Edge = Edge::Empty;
const B: Edge = Edge::Blocked;

#[derive(Debug, Clone)]
pub(crate) struct ConnectionsLayout<'a> {
	ports: Map<(bool, NodeId, PortId), (usize, usize)>, // (x,y)
	connections: Vec<(Connection<'a>, usize)>,          // ((from, to), class)
	edge_field: Betweens<Edge>,
	width: usize,
	height: usize,
	pub(crate) alias_connections: Map<(bool, NodeId, PortId), &'static str>,
	line_types: Map<usize, LineType>,
	line_styles: Map<usize, Style>,
	/// Per-class label text (only connections that called `with_label`). Filled
	/// during [`push_connection`][Self::push_connection]; read during
	/// [`calculate`][Self::calculate] to decide which classes record a midpoint
	/// and during [`render`][Self::render] to draw the text.
	label_texts: Map<usize, &'a str>,
	/// Per-class connection midpoint in canvas coordinates, captured during the
	/// second-pass backtrace in [`calculate`][Self::calculate]. Only populated
	/// for classes that appear in [`label_texts`][Self::label_texts] (i.e. have
	/// a label). An absent entry means "no label → render nothing".
	///
	/// TODO: when two connections' midpoints coincide their labels overwrite
	/// each other. Stacking/offsetting is out of scope for Step 8.
	labels: Map<usize, (usize, usize)>,
	/// Routing failures detected during [`calculate`][Self::calculate], drained
	/// into `NodeGraph::diagnostics` by the caller.
	pub(crate) diagnostics: Vec<Diagnostic>,
}

impl<'a> ConnectionsLayout<'a> {
	pub(crate) fn new(width: usize, height: usize) -> Self {
		Self {
			ports: Map::new(),
			connections: Vec::new(),
			edge_field: Betweens::new(width, height),
			width,
			height,
			alias_connections: Map::new(),
			line_types: Map::new(),
			line_styles: Map::new(),
			label_texts: Map::new(),
			labels: Map::new(),
			diagnostics: Vec::new(),
		}
	}

	pub(crate) fn push_connection(&mut self, connection: (Connection<'a>, usize)) {
		// Record the label text (if any) for this class so that `calculate`
		// knows to capture a midpoint and `render` knows to draw it. The label
		// is borrowed for lifetime 'a (the NodeGraph's lifetime), which is
		// independent of the `connection` tuple itself — so we can read it
		// before moving the tuple.
		let label: Option<&'a str> = connection.0.label;
		let class = connection.1;
		if let Some(label) = label {
			self.label_texts.insert(class, label);
		}
		self.connections.push(connection)
	}

	pub(crate) fn insert_port(
		&mut self,
		is_input: bool,
		node: NodeId,
		port: PortId,
		pos: (usize, usize),
	) {
		self.ports.insert((is_input, node, port), pos);
	}

	pub(crate) fn block_zone(&mut self, area: Rect) {
		for x in 0..area.width {
			for y in 0..area.height {
				let cx = (x + area.x) as usize;
				let cy = (y + area.y) as usize;
				// Guard: skip cells that fall outside the canvas. East indexes
				// horizontal[cy][cx+1] (needs cy<height && cx<width) and South
				// indexes vertical[cy+1][cx] (needs cy<height && cx<width).
				if cx >= self.width || cy >= self.height {
					continue;
				}
				if x != area.width - 1 {
					self.edge_field[((cx, cy), Direction::East).into()] = Edge::Blocked;
				}
				if y != area.height - 1 {
					self.edge_field[((cx, cy), Direction::South).into()] = Edge::Blocked;
				}
			}
		}
	}

	/// Block the edges that cross the port's flow axis so routed connections
	/// don't zig-zag through a port. For **horizontal** ports (the connection
	/// runs along x) we block the vertical (North/South) edges; for **vertical**
	/// ports (connection runs along y) we block the horizontal (East/West) edges.
	pub(crate) fn block_port(&mut self, coord: (usize, usize), vertical: bool) {
		// Guard: edges index into grids sized width×(height+1) or
		// (width+1)×height; both need coord.0<width && coord.1<height.
		if coord.0 >= self.width || coord.1 >= self.height {
			return;
		}
		if vertical {
			self.edge_field[(coord, Direction::East).into()] = Edge::Blocked;
			self.edge_field[(coord, Direction::West).into()] = Edge::Blocked;
		} else {
			self.edge_field[(coord, Direction::North).into()] = Edge::Blocked;
			self.edge_field[(coord, Direction::South).into()] = Edge::Blocked;
		}
	}

	pub(crate) fn calculate(&mut self, direction: crate::FlowDirection) {
		let mut idx_next_alias = 0;
		'outer: for ea_conn in &self.connections {
			self.line_types.insert(ea_conn.1, ea_conn.0.line_type());
			self.line_styles.insert(ea_conn.1, ea_conn.0.line_style());
			let start = (
				self.ports[&(false, ea_conn.0.from_node, ea_conn.0.from_port)],
				direction.main_out_direction(),
			);
			let goal = (
				self.ports[&(true, ea_conn.0.to_node, ea_conn.0.to_port)],
				direction.main_in_direction(),
			);
			if start.0.0 > self.edge_field.width || start.0.1 > self.edge_field.height {
				continue;
			}
			if goal.0.0 > self.edge_field.width || goal.0.1 > self.edge_field.height {
				continue;
			}
			//println!("drawing connection {start:?} to {goal:?}");
			let mut frontier = BinaryHeap::new();
			let mut came_from = Betweens::<Option<_>>::new(self.width, self.height);
			let mut cost = Betweens::<isize>::new(self.width, self.height);
			frontier.push(((0, 0), start));
			let mut count = 0;
			while let Some((_, current)) = frontier.pop() {
				count += 1;
				if count > SEARCH_TIMEOUT {
					break;
				}
				if current == goal {
					break;
				}
				for ea_nei in neighbors(current.0, self.width, self.height) {
					let ea_edge = ea_nei.into();
					let current_cost = cost[current.into()];
					//println!("{current_cost}");
					let new_cost = current_cost.saturating_add(
						self.calc_cost(current, ea_nei, start.0, goal.0, ea_conn.1),
					);
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
			// first pass: mark connections that didnt reach the goal
			let mut next = goal;
			loop {
				if next == start {
					break;
				}
				if let Some(from) = came_from[next.into()] {
					next = from;
				} else {
					// routing failed (search timed out or found no path):
					// record a structured diagnostic, then fall back to an alias
					// character so the connection still has *some* on-screen
					// representation.
					log::warn!(
						"routing failed: no path found \
						 (from_node={}, from_port={}, to_node={}, to_port={})",
						ea_conn.0.from_node,
						ea_conn.0.from_port,
						ea_conn.0.to_node,
						ea_conn.0.to_port,
					);
					self.diagnostics.push(Diagnostic::RoutingFailed {
						from_node: ea_conn.0.from_node,
						from_port: ea_conn.0.from_port,
						to_node: ea_conn.0.to_node,
						to_port: ea_conn.0.to_port,
					});
					// register alias character
					let alias = *self
						.alias_connections
						.entry((false, ea_conn.0.from_node, ea_conn.0.from_port))
						.or_insert_with(|| {
							let a = ALIAS_CHARS[idx_next_alias % ALIAS_CHARS.len()];
							idx_next_alias += 1;
							a
						});
					self.alias_connections
						.insert((true, ea_conn.0.to_node, ea_conn.0.to_port), alias);
					continue 'outer;
				}
			}

			// second pass: collect the path sequence (goal → start), then draw
			// the edges along it. The sequence also yields the connection's
			// midpoint (`seq[len/2]`) for label rendering.
			let mut seq: Vec<(usize, usize)> = Vec::new();
			let mut next = goal;
			loop {
				if next == start {
					break;
				}
				seq.push(next.0);
				self.edge_field[next.into()] = Edge::Connection(ea_conn.1);
				next = came_from[next.into()].unwrap();
			}
			// Record the midpoint for label rendering, but ONLY when this
			// connection actually has a label — otherwise the map stays empty
			// and `render` does no label work (keeping label-less rendering
			// byte-for-byte identical to pre-Step-8 behavior).
			if !seq.is_empty() && self.label_texts.contains_key(&ea_conn.1) {
				let mid = seq[seq.len() / 2];
				self.labels.insert(ea_conn.1, mid);
			}
		}
	}

	pub(crate) fn render(&self, area: Rect, buf: &mut Buffer) {
		let bor = |idx: Edge| -> line::Set {
			if let Edge::Connection(idx) = idx {
				self.line_types[&idx].to_line_set()
			} else if idx == Edge::Blocked {
				line::THICK
			} else {
				line::Set {
					vertical: " ",
					horizontal: " ",
					top_right: " ",
					top_left: " ",
					bottom_right: " ",
					bottom_left: " ",
					vertical_left: " ",
					vertical_right: " ",
					horizontal_down: " ",
					horizontal_up: " ",
					cross: " ",
				}
			}
		};

		let get_line_style = |idx: Edge| -> Style {
			if let Edge::Connection(idx) = idx {
				self.line_styles[&idx]
			} else {
				Style::default()
			}
		};
		for y in 0..self.height {
			for x in 0..self.width {
				let pos = (x, y);
				let north = self.edge_field[(pos, Direction::North).into()];
				let south = self.edge_field[(pos, Direction::South).into()];
				let east = self.edge_field[(pos, Direction::East).into()];
				let west = self.edge_field[(pos, Direction::West).into()];
				#[rustfmt::skip]
				let (symbol, line_style) = match (north, south, east, west) {
					(B | E, B | E, B | E, B | E) => continue,
					(n, s, e, w) if n == B || s == B || e == B || w == B => {
						if n == B && s == B && e != E || w != E && e == w {
							(bor(e).horizontal, get_line_style(e))
						} else if e == B && w == B && n != E && s != E && n == s {
							(bor(n).vertical, get_line_style(n))
						} else {
							("*", Style::default())
						}
					}
					(n, E, E, E) => (bor(n).vertical, get_line_style(n)),
					(E, s, E, E) => (bor(s).vertical, get_line_style(s)),
					(E, E, e, E) => (bor(e).horizontal, get_line_style(e)),
					(E, E, E, w) => (bor(w).horizontal, get_line_style(w)),

					(n, s, E, w) if n == s && n == w => (bor(n).vertical_left, get_line_style(n)),
					(n, E, e, w) if n == e && n == w => (bor(n).horizontal_up, get_line_style(n)),
					(n, s, e, E) if n == s && n == e => (bor(n).vertical_right, get_line_style(n)),
					(E, s, e, w) if s == e && s == w => (bor(s).horizontal_down, get_line_style(s)),
					(E, s, E, w) if s == w => (bor(s).top_right, get_line_style(s)),
					(n, E, E, w) if n == w => (bor(n).bottom_right, get_line_style(n)),
					(n, E, e, E) if n == e => (bor(n).bottom_left, get_line_style(n)),
					(E, s, e, E) if s == e => (bor(s).top_left, get_line_style(s)),

					(n, s, E, E) if n == s => (bor(n).vertical, get_line_style(n)),
					(E, E, e, w) if e == w => (bor(e).horizontal, get_line_style(e)),

					(n, s, e, w) if n == s && n == e && n == w => (bor(n).cross, get_line_style(n)),
					// intersections should just be verticals
					(n, s, e, w) if n == s && e == w && n != E && e != E => (bor(n).vertical, get_line_style(n)),
					(_, _, _, _) => ("?", Style::default()),
				};

				if let Some(cell) = buf.cell_mut(Position::new(
					x as u16 + area.left(),
					y as u16 + area.top(),
				)) {
					cell.set_symbol(symbol).set_style(line_style);
				}
			}
		}

		// Render labels on top of the lines. This is the ONLY opt-in new paint
		// added by Step 8: `self.labels` is empty for a label-less graph, so
		// this loop is a complete no-op and the output above is untouched.
		for (class, (mx, my)) in &self.labels {
			let Some(&text) = self.label_texts.get(class) else {
				continue;
			};
			// The label's color: reuse the connection's own line style so the
			// text inherits its fg. The bg is forced to the canvas default
			// (Black) so the characters punch a readable hole through the line
			// underneath instead of blending into it.
			let base_style = self.line_styles.get(class).copied().unwrap_or_default();
			let label_style = Style::default()
				.fg(base_style.fg.unwrap_or(Color::Reset))
				.bg(Color::Black);
			// Write the label horizontally from the midpoint, truncating to the
			// canvas' right edge so we never write out of bounds. Iterate by
			// grapheme-ish char boundaries: ratatui's `set_symbol` takes a &str,
			// and each ASCII char of the label maps to one cell.
			let start_x = *mx as u16 + area.left();
			let y = *my as u16 + area.top();
			let max_x = area.left() as usize + self.width;
			let mut x = start_x;
			for ch in text.chars() {
				let cx = x;
				if (cx as usize) >= max_x {
					break;
				}
				if let Some(cell) = buf.cell_mut(Position::new(cx, y)) {
					let mut buf_str = [0u8; 4];
					let s = ch.encode_utf8(&mut buf_str);
					cell.set_symbol(s).set_style(label_style);
				}
				x = match cx.checked_add(1) {
					Some(v) => v,
					None => break,
				};
			}
		}
	}

	fn heuristic(from: (usize, usize), to: (usize, usize)) -> isize {
		(from.0 as isize - to.0 as isize).pow(2)
			+ (from.1 as isize - to.1 as isize).pow(2)
	}

	fn calc_cost(
		&self,
		current: ((usize, usize), Direction),
		neigh: ((usize, usize), Direction),
		start: (usize, usize),
		end: (usize, usize),
		conn_t: usize,
	) -> isize {
		let conn_t = Edge::Connection(conn_t);
		let north = self.edge_field[(current.0, Direction::North).into()];
		let south = self.edge_field[(current.0, Direction::South).into()];
		let east = self.edge_field[(current.0, Direction::East).into()];
		let west = self.edge_field[(current.0, Direction::West).into()];

		let in_dir = self.edge_field[current.into()];
		// TODO: fix
		if !(in_dir == Edge::Empty || in_dir == conn_t) {
			return isize::MAX;
		}
		//	assert!(in_dir == 0 || in_dir == conn_t); // should only calculate cost if its possible
		let out_dir = self.edge_field[neigh.into()];
		if out_dir == conn_t {
			// already exists
			1
		} else if out_dir == Edge::Empty {
			if north == conn_t || south == conn_t || east == conn_t || west == conn_t {
				// intersecting with an existing connection
				2 // maybe multiply with distances?
			} else {
				let in_is_vert = current.1.is_vertical();
				let out_is_vert = neigh.1.is_vertical();
				let straight = in_is_vert == out_is_vert;
				if straight {
					if north == Edge::Empty
						&& south == Edge::Empty
						&& east == Edge::Empty
						&& west == Edge::Empty
					{
						2
					} else {
						4
					}
				} else {
					// curved
					if north != Edge::Empty
						|| south != Edge::Empty
						|| east != Edge::Empty
						|| west != Edge::Empty
					{
						isize::MAX
					} else {
						let ax = current.0.0 as isize;
						let ay = current.0.1 as isize;
						let sx = start.0 as isize;
						let sy = start.1 as isize;
						let ex = end.0 as isize;
						let ey = end.1 as isize;
						4 + ((ax - sx).pow(2)
							+ (ay - sy).pow(2) + (ax - ex).pow(2)
							+ (ay - ey).pow(2))
					}
				}
			}
		} else {
			isize::MAX
		}
	}
}

fn neighbors(
	pos: (usize, usize),
	width: usize,
	height: usize,
) -> Vec<((usize, usize), Direction)> {
	let mut out = Vec::new();
	if pos.0 < width - 1 {
		out.push(((pos.0 + 1, pos.1), Direction::West));
	}
	if pos.1 < height - 1 {
		out.push(((pos.0, pos.1 + 1), Direction::North));
	}
	if pos.0 > 0 {
		out.push(((pos.0 - 1, pos.1), Direction::East));
	}
	if pos.1 > 0 {
		out.push(((pos.0, pos.1 - 1), Direction::South));
	}
	out
}

use core::ops::{Index, IndexMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeIdx {
	x: usize,
	y: usize,
	is_vertical: bool,
}
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
			Direction::North => Self { x: value.0.0, y: value.0.1, is_vertical: true },
			Direction::South => {
				Self { x: value.0.0, y: value.0.1 + 1, is_vertical: true }
			}
			Direction::East => {
				Self { x: value.0.0 + 1, y: value.0.1, is_vertical: false }
			}
			Direction::West => Self { x: value.0.0, y: value.0.1, is_vertical: false },
		}
	}
}

// the outermost values are unnecessary
#[derive(Debug, Clone)]
struct Betweens<T: Default> {
	horizontal: Vec<Vec<T>>,
	vertical: Vec<Vec<T>>,
	width: usize,
	height: usize,
}
impl<T: Default> Index<EdgeIdx> for Betweens<T> {
	type Output = T;
	fn index(&self, index: EdgeIdx) -> &Self::Output {
		if index.is_vertical {
			&self.vertical[index.y][index.x]
		} else {
			&self.horizontal[index.y][index.x]
		}
	}
}
impl<T: Default> IndexMut<EdgeIdx> for Betweens<T> {
	fn index_mut(&mut self, index: EdgeIdx) -> &mut T {
		if index.is_vertical {
			&mut self.vertical[index.y][index.x]
		} else {
			&mut self.horizontal[index.y][index.x]
		}
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
		for y in 0..(self.height + 1) {
			for x in 0..self.width {
				print!("{} ", "-".repeat(width));
				f(&self.vertical[y][x]);
			}
			println!("{}", "-".repeat(width));
			if y < self.height {
				for x in 0..(self.width + 1) {
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
