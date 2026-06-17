use ratatui::layout::Position;

use super::*;

const MARGIN: u16 = 5;

#[derive(Debug)]
pub struct NodeGraph<'a> {
	nodes: Vec<NodeLayout<'a>>,
	connections: Vec<Connection>,
	placements: Map<usize, Rect>,
	conn_layout: ConnectionsLayout,
	width: usize,
	/// Non-fatal problems detected during the last [`calculate`][Self::calculate]
	/// (unreachable nodes, ignored bad connections, unrouted connections).
	/// Cleared at the start of each `calculate`.
	diagnostics: Vec<Diagnostic>,
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
			diagnostics: Vec::new(),
		}
	}

	pub fn calculate(&mut self) {
		self.placements.clear();
		self.diagnostics.clear();

		// filter out connections that reference invalid node indices. these
		// would otherwise panic on indexing later. we keep `self.connections`
		// untouched and only work off this local slice for the rest of
		// calculate().
		let valid_conns: Vec<Connection> = self
			.connections
			.iter()
			.copied()
			.filter(|ea| {
				let n = self.nodes.len();
				if ea.from_node >= n || ea.to_node >= n {
					log::warn!(
						"skipping connection: node index out of bounds \
						 (from_node={}, to_node={}, node_count={})",
						ea.from_node,
						ea.to_node,
						n
					);
					self.diagnostics.push(Diagnostic::InvalidConnectionRef {
						from_node: ea.from_node,
						to_node: ea.to_node,
					});
					false
				} else {
					true
				}
			})
			.collect();

		// find root nodes
		let mut roots: Set<_> = (0..self.nodes.len()).collect();
		for ea_connection in valid_conns.iter() {
			roots.remove(&ea_connection.from_node);
		}

		// place them and their children (recursively)
		let mut main_chain = Vec::new();
		let mut visited = Set::new();
		for ea_root in roots {
			self.place_node(ea_root, 0, 0, &mut main_chain, &mut visited, &valid_conns);
			assert!(main_chain.is_empty());
		}

		// calculate connections (eventually, this should be done during node
		// placement, but thats really complicated and i dont wanna deal with that
		// right now. essentially, adding non-trivial connections nudges nodes,
		// and nudging nodes nudges existing connections.)
		let mut conn_map = Map::<(usize, usize), usize>::new();
		let mut next_idx = 1;
		for ea_conn in valid_conns.iter() {
			// a connection may reference a node that never got placed (e.g. one
			// that was only reachable through a cycle that broke placement).
			// skip those defensively instead of indexing into placements.
			let (Some(&a_pos), Some(&b_pos)) =
				(self.placements.get(&ea_conn.from_node), self.placements.get(&ea_conn.to_node))
			else {
				log::warn!(
					"skipping connection layout: endpoint not placed \
					 (from_node={}, to_node={})",
					ea_conn.from_node,
					ea_conn.to_node
				);
				continue;
			};
			// NOTE: don't forget that left and right are swapped.
			// defensively clamp port offsets so an out-of-range port number
			// can't draw outside the node's frame.
			let from_port = clamp_port(ea_conn.from_port, a_pos.height);
			let to_port = clamp_port(ea_conn.to_port, b_pos.height);
			let a_point = (
				self.width.saturating_sub(a_pos.left().into()),
				a_pos.top() as usize + from_port + 1,
			);
			let b_point = (
				self.width.saturating_sub(b_pos.right() as usize + 1),
				b_pos.top() as usize + to_port + 1,
			);
			self.conn_layout.insert_port(
				false,
				ea_conn.from_node,
				ea_conn.from_port,
				a_point,
			);
			self.conn_layout.insert_port(true, ea_conn.to_node, ea_conn.to_port, b_point);
			let key = (ea_conn.from_node, ea_conn.from_port);
			conn_map.entry(key).or_insert_with(|| {
				let idx = next_idx;
				next_idx += 1;
				idx
			});
			self.conn_layout.push_connection((*ea_conn, conn_map[&key]));
			self.conn_layout.block_port(a_point);
			self.conn_layout.block_port(b_point);
		}
		for mut ea_placement in self.placements.values().cloned() {
			ea_placement.x =
				(self.width as u16).saturating_sub(ea_placement.x + ea_placement.width);
			self.conn_layout.block_zone(ea_placement);
		}
		self.conn_layout.calculate();

		// pull routing failures (RoutingFailed) up from the connection layout
		// into the graph-level diagnostics. `append` moves conn_layout's buffer
		// in wholesale and leaves it empty, which is fine — conn_layout's
		// diagnostics aren't read again after this point and are repopulated
		// on the next calculate().
		self.diagnostics.append(&mut self.conn_layout.diagnostics);

		// any node that never got a placement is unreachable (not on any root's
		// upstream chain — e.g. a pure cycle or an isolated node). record it.
		for idx in 0..self.nodes.len() {
			if !self.placements.contains_key(&idx) {
				log::warn!("unreachable node not placed (node={idx})");
				self.diagnostics.push(Diagnostic::UnplacedNode { node: idx });
			}
		}
	}

	/// Non-fatal problems detected during the most recent
	/// [`calculate`][Self::calculate]: unreachable (unplaced) nodes, ignored
	/// out-of-bounds connections, and connections that couldn't be routed.
	///
	/// The slice is cleared at the start of each `calculate`, so it always
	/// reflects the latest run. Each entry is also emitted via `log::warn!`.
	pub fn diagnostics(&self) -> &[Diagnostic] {
		&self.diagnostics
	}

	/// ATTENTION: x_offs works in the opposite direction (higher values are
	/// further left) and y_offs is the same as tui (higher values are further
	/// down)
	fn place_node(
		&mut self,
		idx_node: usize,
		x: u16,
		y: u16,
		main_chain: &mut Vec<usize>,
		visited: &mut Set<usize>,
		conns: &[Connection],
	) {
		// cycle guard: if this node was already placed (reachable again through
		// a cycle), don't re-place or recurse. this is what prevents stack
		// overflows on root-reachable cycles.
		if self.placements.contains_key(&idx_node) || !visited.insert(idx_node) {
			return;
		}

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
					continue 'outer;
				}
			}
			break;
		}
		for ea_node in main_chain.iter() {
			let y = self.placements[ea_node].y.max(rect_me.y);
			self.placements.get_mut(ea_node).unwrap().y = y;
		}
		self.placements.insert(idx_node, rect_me);

		// find children and order them
		let mut y = y;
		main_chain.push(idx_node);
		for ea_child in get_upstream(conns, idx_node) {
			if self.placements.contains_key(&ea_child.from_node) {
				// nudge it (if necessary). a fresh in_progress set per top-level
				// nudge tracks the recursion stack to break cycles without
				// blocking legitimate re-nudges.
				let mut in_progress = Set::new();
				self.nudge(
					ea_child.from_node,
					rect_me.x + rect_me.width + MARGIN,
					&mut in_progress,
					conns,
				);
			} else {
				// place it
				self.place_node(
					ea_child.from_node,
					x + rect_me.width + MARGIN,
					y,
					main_chain,
					visited,
					conns,
				);
				main_chain.clear();
				// child may not have been placed (e.g. it's part of a cycle);
				// only advance y if it actually got a rect.
				if let Some(child_rect) = self.placements.get(&ea_child.from_node) {
					y += child_rect.height;
				}
			}
		}
		main_chain.pop();
	}

	fn nudge(
		&mut self,
		idx_node: usize,
		x: u16,
		in_progress: &mut Set<usize>,
		conns: &[Connection],
	) {
		// cycle guard: break only if this node is already on the current
		// recursion stack (i.e. we're inside a cycle). a node legitimately
		// nudged twice through different paths must still be processed, so we
		// use a per-recursion-stack set, not a global visited set.
		if !in_progress.insert(idx_node) {
			return;
		}
		let rect_me = self.placements[&idx_node];
		if rect_me.x < x {
			self.placements.get_mut(&idx_node).unwrap().x = x;
			for ea_child in get_upstream(conns, idx_node) {
				// the child must already be placed for nudging to make sense;
				// skip defensively if not.
				if self.placements.contains_key(&ea_child.from_node) {
					self.nudge(ea_child.from_node, x + rect_me.width + MARGIN, in_progress, conns);
				}
			}
		}
		in_progress.remove(&idx_node);
	}

	pub fn split(&self, area: Rect) -> Vec<Rect> {
		(0..self.nodes.len())
			.map(|idx_node| {
				self.placements
					.get(&idx_node)
					.map(|pos| {
						if pos.right() > area.width || pos.bottom() > area.height {
							return Rect { x: 0, y: 0, width: 0, height: 0 };
						}
						let mut pos = *pos;
						pos.x = area.width - pos.right() + area.x;
						pos.y += area.y;
						pos.inner(Margin { horizontal: 1, vertical: 1 })
					})
					.unwrap_or_default()
			})
			.collect()
	}
}

fn get_upstream(conns: &[Connection], idx_node: usize) -> Vec<Connection> {
	// find children and order them
	let mut upstream: Vec<_> =
		conns.iter().filter(|ea| ea.to_node == idx_node).copied().collect();
	upstream.sort_by_key(|a| a.to_port);
	upstream
}

fn get_downstream(conns: &[Connection], idx_node: usize) -> Vec<Connection> {
	// find parents and order them
	let mut downstream: Vec<_> =
		conns.iter().filter(|ea| ea.from_node == idx_node).copied().collect();
	downstream.sort_by_key(|a| a.from_port);
	downstream
}

/// Clamp a port index into the valid range of inner rows for a node of the
/// given height. Ports are used as a y-offset into the node's frame; an
/// out-of-range value would draw outside the frame (or past the buffer). The
/// usable inner rows are `0..=height-2` (height includes the top/bottom
/// borders), so the largest valid offset is `height-2`.
fn clamp_port(port: usize, node_height: u16) -> usize {
	let max = (node_height as usize).saturating_sub(2);
	port.min(max)
}

impl<'a> ratatui::widgets::StatefulWidget for NodeGraph<'a> {
	// eventually, this will contain stuff like view position
	//	type State = NodeGraphState;
	type State = ();

	fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
		// draw connections
		self.conn_layout.render(area, buf);

		// draw nodes
		'node: for (idx_node, ea_node) in self.nodes.into_iter().enumerate() {
			if let Some(mut pos) = self.placements.get(&idx_node).copied() {
				if pos.right() > area.width || pos.bottom() > area.height {
					continue 'node;
				}
				// draw box
				pos.x = area.left() + area.width - pos.right();
				pos.y += area.top();
				let block = ea_node.block();
				block.render(pos, buf);
				// draw connection ports
				for ea_conn in get_upstream(&self.connections, idx_node) {
					// clamp port so the offset stays within the node frame
					let to_port = clamp_port(ea_conn.to_port, pos.height) as u16;
					// draw connection alias
					if let Some(alias_char) = self.conn_layout.alias_connections.get(&(
						true,
						idx_node,
						ea_conn.to_port,
					)) {
						let y = pos.top() + to_port + 1;
						if pos.left() > 0 && y < area.height {
							if let Some(cell) = buf.cell_mut(Position::new(pos.left() - 1, y)) {
								cell.set_symbol(alias_char).set_style(
									Style::default()
										.add_modifier(Modifier::BOLD)
										.bg(Color::Red),
								);
							}
						}
					}

					// draw port
					if let Some(cell) = buf.cell_mut(Position::new(pos.left(), pos.top() + to_port + 1))
					{
						cell.set_symbol(conn_symbol(
							true,
							ea_node.border_type(),
							ea_conn.line_type(),
						));
					}
				}
				for ea_conn in get_downstream(&self.connections, idx_node) {
					// clamp port so the offset stays within the node frame
					let from_port = clamp_port(ea_conn.from_port, pos.height) as u16;
					// draw connection alias
					if let Some(alias_char) = self.conn_layout.alias_connections.get(&(
						false,
						idx_node,
						ea_conn.from_port,
					)) {
						if let Some(cell) =
							buf.cell_mut(Position::new(pos.right(), pos.top() + from_port + 1))
						{
							cell.set_symbol(alias_char)
								.set_style(
									Style::default().add_modifier(Modifier::BOLD).bg(Color::Red),
								);
						}
					}

					// draw port
					if let Some(cell) = buf.cell_mut(Position::new(
						pos.right() - 1,
						pos.top() + from_port + 1,
					)) {
						cell.set_symbol(conn_symbol(
							false,
							ea_node.border_type(),
							ea_conn.line_type(),
						));
					}
				}
			} else {
				buf.set_string(
					0,
					idx_node as u16,
					format!("{idx_node}"),
					Style::default(),
				);
			}
		}
	}
}
