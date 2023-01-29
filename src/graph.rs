use super::*;

const MARGIN: u16 = 5;

#[derive(Debug)]
pub struct NodeGraph<'a> {
	nodes: Vec<NodeLayout<'a>>,
	connections: Vec<Connection>,
	placements: Map<usize, Rect>,
	pub conn_layout: ConnectionsLayout,
//	conn_layout: Map<Connection, ConnectionLayout>,
	/// <(is_input, x, y), rendered_char>
	width: usize,
}

impl<'a> NodeGraph<'a> {
	pub fn new(
		nodes: Vec<NodeLayout<'a>>,
		connections: Vec<Connection>,
		width: usize,
		height: usize,
	) -> Self {
		Self {
			nodes,
			connections,
			conn_layout: ConnectionsLayout::new(width, height),
			placements: Default::default(),
			width,
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
		let mut conn_map = Map::<(usize, usize), usize>::new();
		let mut next_idx = 1;
		for ea_conn in self.connections.iter() {
			let a_pos = self.placements[&ea_conn.from_node];
			let b_pos = self.placements[&ea_conn.to_node];
			// NOTE: don't forget that left and right are swapped
			let a_point = (self.width - a_pos.left() as usize, a_pos.top() as usize + ea_conn.from_port + 1);
			let b_point = (self.width - b_pos.right() as usize - 1, b_pos.top() as usize + ea_conn.to_port + 1);
			self.conn_layout.insert_port(false, ea_conn.from_node, ea_conn.from_port, a_point);
			self.conn_layout.insert_port(true,  ea_conn.to_node,   ea_conn.to_port,   b_point);
			let key = (ea_conn.from_node, ea_conn.from_port);
			if !conn_map.contains_key(&key) {
				conn_map.insert(key, next_idx);
				next_idx += 1;
			}
			self.conn_layout.push_connection((*ea_conn, conn_map[&key]));
			self.conn_layout.block_port(a_point);
			self.conn_layout.block_port(b_point);
		}
		for mut ea_placement in self.placements.values().cloned() {
			ea_placement.x = self.width as u16 - ea_placement.x - ea_placement.width;
			self.conn_layout.block_zone(ea_placement);
		}
		self.conn_layout.calculate();

		/*
      // old connection algorithm
		self.connections.sort_by(|a,b| {
			match a.from_node.cmp(&b.from_node) {
				std::cmp::Ordering::Equal => a.from_port.cmp(&b.from_node),
				other => other,
			}
		});
		let mut idx_alias = 0;
		'conn: for ea_conn in self.connections.iter().copied() {
			let a_pos = self.placements[&ea_conn.from_node];
			let b_pos = self.placements[&ea_conn.to_node];
			// NOTE: don't forget that left and right are swapped
			let a_point = (a_pos.left(), a_pos.top() + ea_conn.from_port as u16 + 1);
			let b_point = (b_pos.right() + 1, b_pos.top() + ea_conn.to_port as u16 + 1);
			// check for intersections
			{
				let conn_bbox = Rect {
					x: a_point.0.min(b_point.0),
					y: a_point.1.min(b_point.1),
					width: a_point.0.abs_diff(b_point.0),
					height: a_point.1.abs_diff(b_point.1),
				};
				for ea_node in self.placements.values() {
					if conn_bbox.intersects(*ea_node) {
						let from = (false, ea_conn.from_node, ea_conn.from_port);
						let to = (true, ea_conn.to_node, ea_conn.to_port);
						if let Some(alias) = self.alias_connections.get(&from) {
							self.alias_connections.insert(to, *alias);
						}
						else {
							let alias = ALIAS_CHARS[idx_alias];
							idx_alias += 1;
							self.alias_connections.insert(from, alias);
							self.alias_connections.insert(to, alias);
						}
						continue 'conn
					}
				}
			};
			let layout = self.conn_layout.entry(ea_conn)
				.or_insert(ConnectionLayout::new(a_point));
			if a_point.0 < b_point.0 {
				debug!("skipped due to reversed direction {ea_conn:?}");
				continue
			}
			if a_point.1 != b_point.1 { // different heights
				let midpoint = (a_point.0 + b_point.0)/2;
				if midpoint != a_point.0 {
					// right
					layout.push((Direction::East, a_point.0 - midpoint));
				}
				// up/down
				if a_point.1 > b_point.1 {
					layout.push((Direction::North, a_point.1 - b_point.1));
				}
				else {
					layout.push((Direction::South, b_point.1 - a_point.1));
				}
				if midpoint != b_point.0 {
					// right
					layout.push((Direction::East, midpoint - b_point.0));
				}
			}
			else if a_point.0 != b_point.0 {
				layout.push((Direction::East, a_point.0 - b_point.0));
			}
			// `layout.points` should be empty if `a_point` and `b_point` are the same
		}
		*/
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
		//
		// Repeat the for loop until in runs all the way through without any
		// intersections. Surely there's a more efficient way to do this.
		'outer: loop {
			for (_, ea_them) in self.placements.iter() {
				if rect_me.intersects(*ea_them) {
					rect_me.y = rect_me.y.max(ea_them.bottom());
					continue 'outer
				}
			}
			break
		}
		for ea_node in main_chain.iter() {
			let y = self.placements[ea_node].y.max(rect_me.y);
			self.placements.get_mut(ea_node).unwrap().y = y;
		}
		self.placements.insert(idx_node, rect_me);

		// find children and order them
		let mut y = y;
		main_chain.push(idx_node);
		for ea_child in get_upstream(&self.connections, idx_node) {
			if self.placements.contains_key(&ea_child.from_node) {
				// nudge it (if necessary)
				self.nudge(ea_child.from_node, rect_me.x + rect_me.width + MARGIN);
			}
			else {
				// place it
				self.place_node(ea_child.from_node, x + rect_me.width + MARGIN, y, main_chain);
				main_chain.clear();
				y += self.placements[&ea_child.from_node].height;
			}
		}
		main_chain.pop();
	}

	fn nudge(&mut self, idx_node: usize, x: u16) {
		let rect_me = self.placements[&idx_node];
		if rect_me.x < x {
			self.placements.get_mut(&idx_node).unwrap().x = x;
			for ea_child in get_upstream(&self.connections, idx_node) {
				assert!(self.placements.contains_key(&ea_child.from_node));
				self.nudge(ea_child.from_node, x + rect_me.width + MARGIN);
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
				pos.x = area.width - pos.right();
				pos.inner(&Margin { horizontal: 1, vertical: 1 })
			})
			.unwrap_or_default()
		}).collect()
	}
}

fn get_upstream(conns: &Vec<Connection>, idx_node: usize) -> Vec<Connection> {
	// find children and order them
	let mut upstream: Vec<_> = conns.iter()
		.filter(|ea| { ea.to_node == idx_node })
		.copied()
		.collect();
	upstream.sort_by(|a,b| a.to_port.cmp(&b.to_port));
	upstream
}

fn get_downstream(conns: &Vec<Connection>, idx_node: usize) -> Vec<Connection> {
	// find parents and order them
	let mut downstream: Vec<_> = conns.iter()
		.filter(|ea| { ea.from_node == idx_node })
		.copied()
		.collect()
	;
	downstream.sort_by(|a,b| a.from_port.cmp(&b.from_port));
	downstream
}

impl<'a> tui::widgets::StatefulWidget for NodeGraph<'a> {
	// eventually, this will contain stuff like view position
//	type State = NodeGraphState;
	type State = ();

	fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
		self.conn_layout.render(buf);
		/*
		// draw connections
		'conn: for ea_layout in self.conn_layout.values() {
			let symbols = BorderType::line_symbols(ea_layout.border());
			let mut current_position = ea_layout.start_pos();
			let mut last_dir = Direction::East;
			use Direction::*;
			for (ea_direction, ea_distance) in ea_layout.points() {
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
					.set_style(ea_layout.style());
				match ea_direction {
					Direction::East => {
						for idx in 1..*ea_distance {
							if current_position.0 - idx > area.width || current_position.1 >= area.bottom() { continue 'conn }
							buf.get_mut(area.width - (current_position.0 - idx), current_position.1)
								.set_symbol(symbols.horizontal)
								.set_style(ea_layout.style());
						}
						current_position.0 -= ea_distance;
					}
					Direction::West => {
						for idx in 1..*ea_distance {
							if current_position.0 + idx > area.width || current_position.1 >= area.bottom() { continue 'conn }
							buf.get_mut(area.width - (current_position.0 + idx), current_position.1)
								.set_symbol(symbols.horizontal)
								.set_style(ea_layout.style());
						}
						current_position.0 += ea_distance;
					}
					Direction::North => {
						for idx in 1..*ea_distance {
							if current_position.0 > area.width || current_position.1 - idx >= area.bottom() { continue 'conn }
							buf.get_mut(area.width - current_position.0, current_position.1 - idx)
								.set_symbol(symbols.vertical)
								.set_style(ea_layout.style());
						}
						current_position.1 -= ea_distance;
					}
					Direction::South => {
						for idx in 1..*ea_distance {
							if current_position.0 > area.width || current_position.1 + idx >= area.bottom() { continue 'conn }
							buf.get_mut(area.width - current_position.0, current_position.1 + idx)
								.set_symbol(symbols.vertical)
								.set_style(ea_layout.style());
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
				.set_style(ea_layout.style());
		}
		*/

		// draw nodes
		'node: for (idx_node, ea_node) in self.nodes.into_iter().enumerate() {
			if let Some(mut pos) = self.placements.get(&idx_node).copied() {
				if pos.right() > area.width || pos.bottom() > area.height { continue 'node }
				// draw box
				pos.x = area.width - pos.right();
				let block = Block::default().border_type(ea_node.border).borders(Borders::ALL).title(ea_node.title());
				block.render(pos, buf);
				// draw connection ports
				for ea_conn in get_upstream(&self.connections, idx_node) {
					// draw connection alias
					if let Some(alias_char) = self.conn_layout.alias_connections.get(&(true, idx_node, ea_conn.to_port)) {
						buf.get_mut(pos.left() - 1, pos.top() + ea_conn.to_port as u16 + 1)
							.set_symbol(alias_char)
							.set_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::Red))
						;
					}

					// draw port
					buf.get_mut(pos.left(), pos.top() + ea_conn.to_port as u16 + 1)
						.set_symbol(conn_symbol(true, ea_node.border, BorderType::Double))
					;
				}
				for ea_conn in get_downstream(&self.connections, idx_node) {
					// draw connection alias
					if let Some(alias_char) = self.conn_layout.alias_connections.get(&(false, idx_node, ea_conn.from_port)) {
						buf.get_mut(pos.right(), pos.top() + ea_conn.from_port as u16 + 1)
							.set_symbol(alias_char)
							.set_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::Red))
						;
					}

					// draw port
					buf.get_mut(pos.right() - 1, pos.top() + ea_conn.from_port as u16 + 1)
						.set_symbol(conn_symbol(false, ea_node.border, BorderType::Double))
					;
				}
			}
			else {
				buf.set_string(0, idx_node as u16, format!("{idx_node}"), Style::default());
			}
		}
	}
}
