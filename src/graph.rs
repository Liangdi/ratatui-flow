use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::widgets::Widget;

use super::*;
use crate::id::{NodeId, PortId};

const MARGIN: u16 = 5;

#[derive(Debug)]
pub struct NodeGraph<'a> {
	nodes: Vec<NodeLayout<'a>>,
	connections: Vec<Connection>,
	placements: Map<NodeId, Rect>,
	conn_layout: ConnectionsLayout,
	width: usize,
	height: usize,
	/// Off-screen canvas holding the full graph's borders/ports/connections
	/// (no node *content*) after [`calculate`][Self::calculate]. Used by
	/// [`NodeGraphView`] to blit the visible window onto the screen.
	canvas: Buffer,
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
		let canvas = Buffer::empty(Rect {
			x: 0,
			y: 0,
			width: width as u16,
			height: height as u16,
		});
		Self {
			nodes,
			connections,
			conn_layout: ConnectionsLayout::new(width, height),
			placements: Default::default(),
			width,
			height,
			canvas,
			diagnostics: Vec::new(),
		}
	}

	/// The full-graph canvas (borders/ports/connections only, no content)
	/// rendered during [`calculate`][Self::calculate]. Width/height match the
	/// values passed to [`new`][Self::new].
	///
	/// Exposed so [`NodeGraphView`] can blit a scrolled window from it.
	pub(crate) fn canvas(&self) -> &Buffer {
		&self.canvas
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
				if ea.from_node.0 as usize >= n || ea.to_node.0 as usize >= n {
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
		let mut roots: Set<_> = (0..self.nodes.len()).map(|i| NodeId(i as u32)).collect();
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
		let mut conn_map = Map::<(NodeId, PortId), usize>::new();
		let mut next_idx = 1;
		for ea_conn in valid_conns.iter() {
			// a connection may reference a node that never got placed (e.g. one
			// that was only reachable through a cycle that broke placement).
			// skip those defensively instead of indexing into placements.
			let (Some(&a_pos), Some(&b_pos)) = (
				self.placements.get(&ea_conn.from_node),
				self.placements.get(&ea_conn.to_node),
			) else {
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
			let from_port = clamp_port(ea_conn.from_port.0 as usize, a_pos.height);
			let to_port = clamp_port(ea_conn.to_port.0 as usize, b_pos.height);
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
			let node_id = NodeId(idx as u32);
			if !self.placements.contains_key(&node_id) {
				log::warn!("unreachable node not placed (node={})", node_id);
				self.diagnostics.push(Diagnostic::UnplacedNode { node: node_id });
			}
		}

		// Re-render the whole graph's borders/ports/connections (no node
		// *content*) into the off-screen canvas. This runs once per layout; the
		// viewport widget then just blits a scrolled window from it each frame
		// instead of re-laying-out.
		//
		// Render into a fresh local buffer first, then assign: `render_to` borrows
		// `&self` (for nodes/placements/connections), so we can't hand it
		// `&mut self.canvas` at the same time.
		let canvas_rect = Rect {
			x: 0,
			y: 0,
			width: self.width as u16,
			height: self.height as u16,
		};
		let mut canvas = Buffer::empty(canvas_rect);
		self.render_to(canvas_rect, &mut canvas);
		self.canvas = canvas;
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
		idx_node: NodeId,
		x: u16,
		y: u16,
		main_chain: &mut Vec<NodeId>,
		visited: &mut Set<NodeId>,
		conns: &[Connection],
	) {
		// cycle guard: if this node was already placed (reachable again through
		// a cycle), don't re-place or recurse. this is what prevents stack
		// overflows on root-reachable cycles.
		if self.placements.contains_key(&idx_node) || !visited.insert(idx_node) {
			return;
		}

		// place the node
		let size_me = self.nodes[idx_node.0 as usize].size;
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
		idx_node: NodeId,
		x: u16,
		in_progress: &mut Set<NodeId>,
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
					self.nudge(
						ea_child.from_node,
						x + rect_me.width + MARGIN,
						in_progress,
						conns,
					);
				}
			}
		}
		in_progress.remove(&idx_node);
	}

	pub fn split(&self, area: Rect) -> Vec<Rect> {
		(0..self.nodes.len())
			.map(|idx_node| {
				self.placements
					.get(&NodeId(idx_node as u32))
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

	/// Like [`split`][Self::split], but returns each node's content rect in
	/// **screen** coordinates for a scrolled viewport.
	///
	/// Each rect is the node's canvas-rect (mirrored, inner border removed),
	/// translated by `-viewport.offset + area.origin`, then clipped to `area`.
	/// A node fully scrolled off-screen yields a 0×0 rect (render it only when
	/// `width > 0 && height > 0`).
	///
	/// Typical per-frame usage:
	/// ```ignore
	/// let zones = graph.split_viewport(view_area, &viewport);
	/// for (i, z) in zones.iter().enumerate() {
	///     if z.width > 0 && z.height > 0 {
	///         f.render_widget(my_content[i], *z);
	///     }
	/// }
	/// f.render_widget(NodeGraphView::new(&graph).viewport(viewport), view_area);
	/// ```
	pub fn split_viewport(&self, area: Rect, viewport: &Viewport) -> Vec<Rect> {
		let canvas_rect = Rect {
			x: 0,
			y: 0,
			width: self.width as u16,
			height: self.height as u16,
		};
		let (ox, oy) = viewport.offset;
		self.split(canvas_rect)
			.into_iter()
			.map(|z| {
				// A node whose entire rect sits above/left of the viewport
				// (canvas right edge <= offset.x, or bottom edge <= offset.y) is
				// fully scrolled off and must become 0×0. `saturating_sub` alone
				// would clamp it to the screen's top-left edge and keep its size,
				// wrongly painting a partial node in the corner — so detect that
				// case first.
				if z.right() <= ox || z.bottom() <= oy {
					return Rect::default();
				}
				let mut z = z;
				// translate canvas-coord rect to screen coordinates: subtract the
				// viewport offset, then add the screen area's origin.
				z.x = z.x.saturating_sub(ox).saturating_add(area.x);
				z.y = z.y.saturating_sub(oy).saturating_add(area.y);
				// clip to the visible area; `intersection` returns a 0×0 rect when
				// the node is fully off the far edge.
				z.intersection(area)
			})
			.collect()
	}
}

/// Scroll position into a [`NodeGraph`]'s canvas.
///
/// `offset` is the (x, y) coordinate of the viewport's top-left corner within
/// the off-screen canvas. (0, 0) shows the top-left of the graph; increasing x
/// scrolls right, increasing y scrolls down.
///
/// Pass it to [`NodeGraph::split_viewport`] (for node content rects) and
/// [`NodeGraphView`] (for borders/connections).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Viewport {
	/// Top-left corner of the viewport in canvas coordinates.
	pub offset: (u16, u16),
}

impl Viewport {
	/// Create a viewport at offset (0, 0) (top-left of the canvas).
	pub fn new() -> Self {
		Self::default()
	}

	/// Set the viewport's top-left offset in canvas coordinates.
	#[must_use]
	pub fn offset(mut self, x: u16, y: u16) -> Self {
		self.offset = (x, y);
		self
	}
}

impl std::fmt::Display for Viewport {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Viewport({}, {})", self.offset.0, self.offset.1)
	}
}

/// A read-only [`Widget`] that renders a scrolled window of a [`NodeGraph`]'s
/// borders/ports/connections (no node *content*).
///
/// The full graph is rendered once into an off-screen canvas during
/// [`NodeGraph::calculate`]; this widget blits the visible window (determined
/// by its [`Viewport`]) onto the frame each draw. Render your own node content
/// into the rects from [`NodeGraph::split_viewport`] separately.
///
/// ```ignore
/// f.render_widget(NodeGraphView::new(&graph).offset(x, y), area);
/// ```
pub struct NodeGraphView<'a> {
	graph: &'a NodeGraph<'a>,
	viewport: Viewport,
}

impl<'a> NodeGraphView<'a> {
	/// Create a view over `graph` at offset (0, 0).
	pub fn new(graph: &'a NodeGraph<'a>) -> Self {
		Self { graph, viewport: Viewport::default() }
	}

	/// Set the viewport (offset into the canvas).
	#[must_use]
	pub fn viewport(mut self, viewport: Viewport) -> Self {
		self.viewport = viewport;
		self
	}

	/// Convenience: set the viewport offset directly (x, y).
	#[must_use]
	pub fn offset(mut self, x: u16, y: u16) -> Self {
		self.viewport.offset = (x, y);
		self
	}
}

impl Widget for NodeGraphView<'_> {
	fn render(self, area: Rect, buf: &mut Buffer) {
		let canvas = self.graph.canvas();
		let (ox, oy) = self.viewport.offset;
		// blit the visible window of the canvas onto `area`, cell by cell.
		// for each screen cell, look up the canvas cell at (offset + screen-delta)
		// and copy its symbol + style. out-of-canvas cells are skipped.
		for vy in 0..area.height {
			for vx in 0..area.width {
				let sx = ox.saturating_add(vx);
				let sy = oy.saturating_add(vy);
				let src = match canvas.cell(Position::new(sx, sy)) {
					Some(c) => c,
					None => continue,
				};
				// Blank canvas cells are node content areas / background — leave
				// them untouched so content the caller rendered into the
				// `split_viewport` rects shows through. Only copy cells the canvas
				// actually drew (borders / ports / connections).
				if src.symbol() == " " {
					continue;
				}
				if let Some(dst) = buf.cell_mut(Position::new(area.x + vx, area.y + vy)) {
					dst.set_symbol(src.symbol());
					dst.set_style(src.style());
				}
			}
		}
	}
}

fn get_upstream(conns: &[Connection], idx_node: NodeId) -> Vec<Connection> {
	// find children and order them
	let mut upstream: Vec<_> =
		conns.iter().filter(|ea| ea.to_node == idx_node).copied().collect();
	upstream.sort_by_key(|a| a.to_port);
	upstream
}

fn get_downstream(conns: &[Connection], idx_node: NodeId) -> Vec<Connection> {
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

impl<'a> NodeGraph<'a> {
	/// Render the graph's borders/ports/connections (but *not* node content)
	/// into `buf` at `area`. This is the shared implementation behind both the
	/// [`StatefulWidget`] impl (used by `split`-based rendering) and the
	/// off-screen canvas drawn during [`calculate`][Self::calculate] (used by
	/// [`NodeGraphView`]).
	///
	/// Takes `&self` (rather than consuming the widget) so the same graph can
	/// be rendered multiple times — once into the canvas during layout, and
	/// any number of times via the `StatefulWidget` impl for non-viewport use.
	pub(crate) fn render_to(&self, area: Rect, buf: &mut Buffer) {
		// draw connections
		self.conn_layout.render(area, buf);

		// draw nodes
		'node: for (idx_node, ea_node) in self.nodes.iter().enumerate() {
			let idx_node = NodeId(idx_node as u32);
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
					let to_port =
						clamp_port(ea_conn.to_port.0 as usize, pos.height) as u16;
					// draw connection alias
					if let Some(alias_char) = self.conn_layout.alias_connections.get(&(
						true,
						idx_node,
						ea_conn.to_port,
					)) {
						let y = pos.top() + to_port + 1;
						if pos.left() > 0
							&& y < area.height && let Some(cell) =
							buf.cell_mut(Position::new(pos.left() - 1, y))
						{
							cell.set_symbol(alias_char).set_style(
								Style::default()
									.add_modifier(Modifier::BOLD)
									.bg(Color::Red),
							);
						}
					}

					// draw port
					if let Some(cell) =
						buf.cell_mut(Position::new(pos.left(), pos.top() + to_port + 1))
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
					let from_port =
						clamp_port(ea_conn.from_port.0 as usize, pos.height) as u16;
					// draw connection alias
					if let Some(alias_char) = self.conn_layout.alias_connections.get(&(
						false,
						idx_node,
						ea_conn.from_port,
					)) && let Some(cell) = buf
						.cell_mut(Position::new(pos.right(), pos.top() + from_port + 1))
					{
						cell.set_symbol(alias_char).set_style(
							Style::default().add_modifier(Modifier::BOLD).bg(Color::Red),
						);
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
					idx_node.0 as u16,
					format!("{}", idx_node.0),
					Style::default(),
				);
			}
		}
	}
}

impl<'a> ratatui::widgets::StatefulWidget for NodeGraph<'a> {
	// eventually, this will contain stuff like view position
	//	type State = NodeGraphState;
	type State = ();

	fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
		self.render_to(area, buf);
	}
}
