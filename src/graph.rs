use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::widgets::Widget;

use super::*;
use crate::id::{NodeId, PortId};

const MARGIN: u16 = 5;

/// Error returned by [`NodeGraph::add_node_with_id`] when the requested
/// [`NodeId`] already exists in the graph.
///
/// Note: this is **not** pushed into [`NodeGraph::diagnostics`] (which is
/// cleared on every [`calculate`][NodeGraph::calculate]); it is returned as a
/// `Result` so the caller can react immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddNodeError {
	/// The given [`NodeId`] is already in use by an existing node.
	ConflictingId,
}

impl std::fmt::Display for AddNodeError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			AddNodeError::ConflictingId => write!(f, "node id already in use"),
		}
	}
}

impl std::error::Error for AddNodeError {}

/// How [`NodeGraph`] decides where to place each node during
/// [`calculate`][NodeGraph::calculate].
///
/// - [`Auto`](Self::Auto) — the default. The graph runs its built-in recursive
///   layout (`place_node`) starting from the root nodes, flowing in
///   [`direction`][NodeGraph::direction]. This is byte-for-byte identical to
///   the pre-`LayoutMode` behavior.
/// - [`Manual`](Self::Manual) — automatic placement is **skipped entirely**;
///   each node's position is read from the coords set via
///   [`set_position`][NodeGraph::set_position] /
///   [`with_position`][NodeGraph::with_position]. A node with no recorded
///   position becomes an [`UnplacedNode`][Diagnostic::UnplacedNode]
///   diagnostic, exactly like an unreachable node in `Auto` mode.
///
/// Manual coordinates are in the same canvas/logical space the `Auto` layout
/// uses (origin top-left of the off-screen canvas), and are therefore subject
/// to the same `Rtl`/`Btt` main-axis mirroring applied during routing and
/// rendering — i.e. set the coordinate you'd get from
/// [`node_rect`][NodeGraph::node_rect], not the mirrored on-screen one.
/// Manual positions are user state and **survive** `calculate()` (they are
/// not cleared); call [`clear_position`][NodeGraph::clear_position] to drop a
/// node back to "unplaced".
///
/// [`Pinned`][LayoutMode::Pinned] blends the two: some nodes are pinned to
/// fixed coords via [`set_position`][NodeGraph::set_position] (the **same**
/// `manual_positions` map `Manual` reads) and act as immovable anchors, while
/// every other node auto-layouts around them, treating the pinned rects as
/// obstacles it must not overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutMode {
	/// Automatic layout: nodes are placed by the built-in recursive layout
	/// (`place_node`) from the root nodes, flowing in
	/// [`direction`][NodeGraph::direction]. This is the default and matches the
	/// original hard-coded behavior byte-for-byte.
	#[default]
	Auto,
	/// Manual layout: automatic placement is skipped; each node's position is
	/// taken from [`set_position`][NodeGraph::set_position] /
	/// [`with_position`][NodeGraph::with_position]. Nodes without a recorded
	/// position become [`UnplacedNode`][Diagnostic::UnplacedNode] diagnostics.
	Manual,
	/// Pinned layout: a hybrid of [`Auto`](Self::Auto) and
	/// [`Manual`](Self::Manual). Every node present in
	/// [`manual_positions`][NodeGraph::manual_positions] is **pinned** to its
	/// recorded `(x, y)` — its rect is `Rect::new(x, y, size.0, size.1)` and is
	/// treated as an immovable anchor that the internal `place_node` and `nudge`
	/// passes never reposition. All other nodes are auto-laid-out exactly as in
	/// the pinned rects are pre-populated into `placements` before the walk, so
	/// the existing intersection/nudge logic treats them as obstacles and shifts
	/// the auto-placed nodes along the cross axis to avoid overlapping them.
	///
	/// The walk still traverses **through** a pinned node to reach its
	/// neighbors (parents/children): the neighbor's main-axis offset is computed
	/// from the pinned node's fixed far edge, so a chain `A -> pinned -> B`
	/// still flows correctly with `B` placed beyond the pinned rect. Pinned
	/// nodes themselves are never moved, even when an auto-placed neighbor's
	/// main-chain nudge would otherwise push them.
	///
	/// **v1 limitation — overlap-free in common cases only.** The cross-axis
	/// nudge loop shifts nodes away from obstacles one at a time and the
	/// main-chain nudge propagates along the main axis; together these resolve
	/// ordinary, well-spaced graphs without overlap. In pathological configs
	/// (e.g. a pinned rect that fully blocks the only cross-axis lane on a tiny
	/// canvas, or mutually-conflicting pinned positions) an overlap may be
	/// unavoidable. In that case the node is placed at the best-effort
	/// coordinate (the loop is bounded — it never panics, overflows, or spins
	/// forever) and a [`UnplacedNode`][Diagnostic::UnplacedNode] or
	/// [`RoutingFailed`][Diagnostic::RoutingFailed] diagnostic is emitted as
	/// appropriate. `calculate()` always returns.
	Pinned,
}

#[derive(Debug, Clone)]
pub struct NodeGraph<'a> {
	/// Nodes in render order, each tagged with its stable [`NodeId`].
	///
	/// This is a `Vec` of `(id, layout)` pairs (not indexed by id) so that
	/// [`add_node_with_id`] can accept non-contiguous ids. Lookups happen by
	/// scanning (node counts are in the low hundreds); the old "`NodeId(i)` is
	/// the vector index" convention is preserved only for graphs built via
	/// [`new`][Self::new], which assigns `NodeId(0..n)`.
	nodes: Vec<(NodeId, NodeLayout<'a>)>,
	connections: Vec<Connection<'a>>,
	placements: Map<NodeId, Rect>,
	/// User-set top-left coordinates for [`LayoutMode::Manual`], keyed by
	/// [`NodeId`]. These are user state (like [`nodes`][Self::nodes]) and are
	/// **not** cleared by [`calculate`][Self::calculate]; in `Manual` mode each
	/// node with an entry here is placed at `(x, y)`, and nodes without an
	/// entry become [`UnplacedNode`][Diagnostic::UnplacedNode] diagnostics.
	/// Ignored in [`LayoutMode::Auto`] (the auto layout owns placement then).
	manual_positions: Map<NodeId, (u16, u16)>,
	/// How nodes are placed during [`calculate`][Self::calculate]: auto layout
	/// ([`Auto`][LayoutMode::Auto], the default) or explicit coords
	/// ([`Manual`][LayoutMode::Manual]). Defaults to `Auto`, so a graph built
	/// without touching it lays out byte-for-byte as before.
	layout_mode: LayoutMode,
	conn_layout: ConnectionsLayout<'a>,
	width: usize,
	height: usize,
	/// Which way the graph flows (root edge + child growth axis). Defaults to
	/// [`FlowDirection::Rtl`], which is byte-for-byte identical to the
	/// pre-parameterization hard-coded layout.
	direction: FlowDirection,
	/// Auto-incrementing counter for ids handed out by [`add_node`][Self::add_node].
	/// `add_node_with_id` may push ids beyond this counter without bumping it.
	next_id: u32,
	/// True whenever a mutator changed the graph since the last
	/// [`calculate`][Self::calculate]. Mutators do **not** re-run layout
	/// automatically — callers must invoke `calculate` before reading
	/// [`positions`][Self::positions] / [`split`][Self::split] / etc., or the
	/// values will be stale.
	dirty: bool,
	/// Off-screen canvas holding the full graph's borders/ports/connections
	/// (no node *content*) after [`calculate`][Self::calculate]. Used by
	/// [`NodeGraphView`] to blit the visible window onto the screen.
	canvas: Buffer,
	/// Non-fatal problems detected during the last [`calculate`][Self::calculate]
	/// (unreachable nodes, ignored bad connections, unrouted connections).
	/// Cleared at the start of each `calculate`.
	diagnostics: Vec<Diagnostic>,
	/// Style applied to the selected node's border during stateful render (the
	/// hover node gets this style with a `DIM` modifier added). Defaults to bold
	/// yellow fg.
	highlight_style: Style,
	/// When `true`, the `to` (in) port of each connection is drawn as a direction
	/// arrow ([`arrow_symbol`][crate] — ◄/►/▼/…) pointing in the flow direction,
	/// instead of the in-port connection glyph (`┤`/`┴` family). The `from` (out)
	/// port always keeps its `├`/`┬` glyph. **Off by default**; enable with
	/// [`show_arrows`][Self::show_arrows].
	show_arrows: bool,
}

impl<'a> NodeGraph<'a> {
	pub fn new(
		nodes: Vec<NodeLayout<'a>>,
		connections: Vec<Connection<'a>>,
		width: usize,
		height: usize,
	) -> Self {
		let mut graph = Self::empty(width, height);
		// Assign NodeId(0..n) to the incoming nodes so the legacy
		// `rects[i]` / `NodeId(i)` semantics stay intact. We bump `next_id`
		// to `n` so a subsequent `add_node` keeps handing out fresh ids.
		for (i, node) in nodes.into_iter().enumerate() {
			// ids 0..n are guaranteed unique here (graph is empty).
			let _ = graph.add_node_with_id(NodeId(i as u32), node);
		}
		// Same for the supplied connections.
		for conn in connections {
			graph.add_connection(conn);
		}
		graph
	}

	/// Build an empty graph with the given canvas size. Single construction
	/// path shared by [`new`][Self::new] and incremental callers.
	fn empty(width: usize, height: usize) -> Self {
		let canvas = Buffer::empty(Rect {
			x: 0,
			y: 0,
			width: width as u16,
			height: height as u16,
		});
		Self {
			nodes: Vec::new(),
			connections: Vec::new(),
			conn_layout: ConnectionsLayout::new(width, height),
			placements: Default::default(),
			manual_positions: Default::default(),
			layout_mode: LayoutMode::default(),
			width,
			height,
			direction: FlowDirection::default(),
			next_id: 0,
			dirty: true,
			canvas,
			diagnostics: Vec::new(),
			highlight_style: Style::default()
				.add_modifier(Modifier::BOLD)
				.fg(Color::Yellow),
			show_arrows: false,
		}
	}

	// ----- configuration --------------------------------------------------

	/// Set the graph's [`FlowDirection`] (which edge the root anchors to and
	/// which way children flow). The graph is marked dirty; call
	/// [`calculate`][Self::calculate] before reading positions.
	///
	/// Defaults to [`FlowDirection::Rtl`] (the original hard-coded behavior).
	#[must_use]
	pub fn with_direction(mut self, direction: FlowDirection) -> Self {
		self.direction = direction;
		self.dirty = true;
		self
	}

	/// The graph's configured [`FlowDirection`].
	pub fn direction(&self) -> FlowDirection {
		self.direction
	}

	/// Set the highlight [`Style`] applied to the **selected** node's border
	/// during stateful render (see [`FlowState::selection`]). The **hover**
	/// node's border gets this style with an added [`Modifier::DIM`] so the two
	/// states stay visually distinct. Defaults to bold yellow foreground.
	///
	/// Only the border is recolored (the port glyphs on the border keep their
	/// symbols and pick up the highlight's fg/modifier); node content is never
	/// touched.
	#[must_use]
	pub fn highlight_style(self, style: Style) -> Self {
		let mut this = self;
		this.highlight_style = style;
		this
	}

	/// Toggle drawing a **direction arrow** on each connection's `to` (in) port,
	/// pointing in the flow direction (e.g. `◄` for `Rtl`, `▶` for `Ltr`, `▼`
	/// for `Ttb`). **Off by default** — pass `true` to replace the in-port
	/// connection glyph (`┤`/`┴` family) with an arrow so the flow direction
	/// reads at a glance. The `from` (out) port always keeps its `├`/`┬` glyph.
	#[must_use]
	pub fn show_arrows(mut self, on: bool) -> Self {
		self.show_arrows = on;
		self
	}

	/// Mutator counterpart of [`with_direction`][Self::with_direction]: set the
	/// flow direction on an existing graph. The graph is marked dirty; call
	/// [`calculate`][Self::calculate] before reading positions. Useful for
	/// interactive apps that rotate the layout at runtime without rebuilding.
	pub fn set_direction(&mut self, direction: FlowDirection) {
		self.direction = direction;
		self.dirty = true;
	}

	/// Mutator counterpart of [`show_arrows`][Self::show_arrows]: toggle the
	/// connection direction arrows on an existing graph. The graph is marked
	/// dirty so the off-screen canvas is re-rendered with the new setting on the
	/// next [`calculate`][Self::calculate] (matters for the panned/blitted view).
	pub fn set_show_arrows(&mut self, on: bool) {
		self.show_arrows = on;
		self.dirty = true;
	}

	// ----- layout mode ----------------------------------------------------

	/// The graph's current [`LayoutMode`] (auto vs. manual node placement).
	/// Defaults to [`LayoutMode::Auto`], so a graph built without calling
	/// [`set_layout_mode`][Self::set_layout_mode] lays out automatically.
	pub fn layout_mode(&self) -> LayoutMode {
		self.layout_mode
	}

	/// Set the graph's [`LayoutMode`]. The graph is marked dirty; call
	/// [`calculate`][Self::calculate] before reading positions. In
	/// [`Manual`][LayoutMode::Manual] mode the existing
	/// [`manual_positions`][Self::manual_positions] entries are used; in
	/// [`Auto`][LayoutMode::Auto] mode they are ignored (but kept, so toggling
	/// back to `Manual` later restores them).
	pub fn set_layout_mode(&mut self, mode: LayoutMode) {
		self.layout_mode = mode;
		self.dirty = true;
	}

	/// Builder counterpart of [`set_layout_mode`][Self::set_layout_mode].
	#[must_use]
	pub fn with_layout_mode(mut self, mode: LayoutMode) -> Self {
		self.layout_mode = mode;
		self.dirty = true;
		self
	}

	/// Record a manual top-left coordinate `(x, y)` for `id` — used by
	/// [`LayoutMode::Manual`] during [`calculate`][Self::calculate]. The graph
	/// is marked dirty.
	///
	/// The coordinate is the node's **top-left in layout/canvas space** (origin
	/// top-left of the off-screen canvas), the same space
	/// [`node_rect`][Self::node_rect] reports — `Rtl`/`Btt` main-axis mirroring
	/// is applied during routing/render, so set the un-mirrored coordinate. The
	/// node's full frame is `Rect::new(x, y, size.0, size.1)` where `size` is
	/// the [`NodeLayout::size`] (border included).
	///
	/// `id` is **not** validated to exist in the graph: this lets a caller
	/// pre-declare a position before [`add_node_with_id`][Self::add_node_with_id]
	/// inserts the node, and the entry simply has no effect (in `Auto` mode) or
	/// becomes dead state (in `Manual` mode, for an id not in `nodes`) until the
	/// node is added. Manual positions are user state and **survive**
	/// `calculate()`; call [`clear_position`][Self::clear_position] to drop one.
	pub fn set_position(&mut self, id: NodeId, x: u16, y: u16) {
		self.manual_positions.insert(id, (x, y));
		self.dirty = true;
	}

	/// Builder counterpart of [`set_position`][Self::set_position].
	#[must_use]
	pub fn with_position(mut self, id: NodeId, x: u16, y: u16) -> Self {
		self.manual_positions.insert(id, (x, y));
		self.dirty = true;
		self
	}

	/// Drop the manual position for `id`, if any. No-op (does not error) if no
	/// position was recorded. The graph is marked dirty; in
	/// [`Manual`][LayoutMode::Manual] mode the next [`calculate`][Self::calculate]
	/// will report `id` as an [`UnplacedNode`][Diagnostic::UnplacedNode] (if it's
	/// still in `nodes`) since it no longer has explicit coords.
	pub fn clear_position(&mut self, id: NodeId) {
		self.manual_positions.remove(&id);
		self.dirty = true;
	}

	/// All user-set manual positions (via [`set_position`][Self::set_position]
	/// / [`with_position`][Self::with_position]), keyed by [`NodeId`]. Read-only
	/// view intended for editor persistence (save/restore the map across runs)
	/// and for diagnostics. Entries are **not** cleared by
	/// [`calculate`][Self::calculate].
	pub fn manual_positions(&self) -> &Map<NodeId, (u16, u16)> {
		&self.manual_positions
	}

	// ----- mutators -------------------------------------------------------

	/// Append a node, assigning it the next fresh [`NodeId`] (and bumping the
	/// internal counter). The graph is marked dirty; you must call
	/// [`calculate`][Self::calculate] before reading positions.
	///
	/// `node.title`/content strings should outlive the graph; using a
	/// `&'static str` is the common case.
	pub fn add_node(&mut self, node: NodeLayout<'a>) -> NodeId {
		let id = NodeId(self.next_id);
		self.next_id += 1;
		// next_id is monotonically increasing, so the id is guaranteed unique.
		let _ = self.add_node_with_id(id, node);
		id
	}

	/// Append a node with a caller-chosen [`NodeId`]. Returns
	/// [`AddNodeError::ConflictingId`] if `id` is already in use; otherwise the
	/// node is inserted (preserving render order) and the graph is marked dirty.
	///
	/// The id need not be contiguous — this is the path used to support
	/// non-contiguous node ids. The auto-increment counter is bumped past `id`
	/// only if `id` would otherwise collide with a future `add_node` result.
	pub fn add_node_with_id(
		&mut self,
		id: NodeId,
		node: NodeLayout<'a>,
	) -> Result<(), AddNodeError> {
		if self.has_node(id) {
			return Err(AddNodeError::ConflictingId);
		}
		// keep next_id strictly greater than any id ever inserted so a later
		// add_node() can't hand out an id that add_node_with_id already took.
		if id.0 >= self.next_id {
			self.next_id = id.0 + 1;
		}
		self.nodes.push((id, node));
		self.dirty = true;
		Ok(())
	}

	/// Append a [`Connection`]. The graph is marked dirty.
	pub fn add_connection(&mut self, conn: Connection<'a>) {
		self.connections.push(conn);
		self.dirty = true;
	}

	/// Remove the node with the given [`NodeId`] and cascade-delete every
	/// [`Connection`] that references it (on either endpoint). No-op (returns
	/// `false`) if `id` is not in the graph. The graph is marked dirty.
	///
	/// Returns `true` if a node was actually removed.
	pub fn remove_node(&mut self, id: NodeId) -> bool {
		let before = self.nodes.len();
		self.nodes.retain(|(nid, _)| *nid != id);
		let removed = self.nodes.len() != before;
		if removed {
			// cascade: drop any connection touching the removed node.
			self.connections.retain(|c| c.from_node != id && c.to_node != id);
			self.dirty = true;
		}
		removed
	}

	/// Remove the first [`Connection`] matching the given endpoints. No-op
	/// (returns `false`) if no such connection exists. The graph is marked dirty.
	///
	/// Returns `true` if a connection was removed.
	pub fn remove_connection(
		&mut self,
		from: NodeId,
		from_port: PortId,
		to: NodeId,
		to_port: PortId,
	) -> bool {
		let before = self.connections.len();
		if let Some(pos) = self.connections.iter().position(|c| {
			c.from_node == from
				&& c.from_port == from_port
				&& c.to_node == to
				&& c.to_port == to_port
		}) {
			self.connections.remove(pos);
		}
		let removed = self.connections.len() != before;
		if removed {
			self.dirty = true;
		}
		removed
	}

	/// Replace an existing node's [`NodeLayout`] (size/title/border/port-names)
	/// in place, keeping its [`NodeId`] and all connections. Lets an editor resize
	/// or retitle a node without a remove+re-add. Returns
	/// [`Err(ConflictingId)`][AddNodeError::ConflictingId] if `id` is not present
	/// (reuses [`AddNodeError`] for a stable error type); on success the graph is
	/// marked dirty (call [`calculate`] before reading positions). Connections
	/// referencing `id` are untouched.
	///
	/// [`calculate`]: Self::calculate
	pub fn replace_node(
		&mut self,
		id: NodeId,
		node: NodeLayout<'a>,
	) -> Result<(), AddNodeError> {
		match self.nodes.iter().position(|(nid, _)| *nid == id) {
			None => Err(AddNodeError::ConflictingId),
			Some(idx) => {
				self.nodes[idx].1 = node;
				self.dirty = true;
				Ok(())
			}
		}
	}

	// ----- queries / getters ---------------------------------------------

	/// `true` if a node with the given [`NodeId`] exists in the graph.
	pub fn has_node(&self, id: NodeId) -> bool {
		self.nodes.iter().any(|(nid, _)| *nid == id)
	}

	/// All nodes in render (insertion) order, each tagged with its stable
	/// [`NodeId`] and [`NodeLayout`]. Exposed so callers can enumerate the
	/// graph's nodes — build a side panel, cycle a selection, look up a title —
	/// without keeping a parallel structure in sync with
	/// [`add_node`][Self::add_node] / [`remove_node`][Self::remove_node].
	///
	/// The slice is the graph's authoritative node list; mutating the graph
	/// updates it in place. Must be called after [`calculate`][Self::calculate]
	/// if you also intend to read [`positions`][Self::positions].
	pub fn nodes(&self) -> &[(NodeId, NodeLayout<'a>)] {
		&self.nodes
	}

	/// All [`Connection`]s currently in the graph, in insertion order. This is
	/// the authoritative edge list: mutators like
	/// [`remove_node`][Self::remove_node] cascade-delete every connection that
	/// touches the removed node, so reading this after any mutation avoids drift
	/// with a caller-maintained shadow list. Pair with [`Connection::from_node`]
	/// / [`Connection::to_node`] (and the port/label accessors) to inspect an
	/// edge's endpoints.
	pub fn connections(&self) -> &[Connection<'a>] {
		&self.connections
	}

	/// `true` if any connection has endpoints `(from, to)` in that direction
	/// (ports ignored). Cheap scan for "are these two nodes already linked".
	#[must_use]
	pub fn has_connection(&self, from: NodeId, to: NodeId) -> bool {
		self.connections.iter().any(|c| c.from_node == from && c.to_node == to)
	}

	/// All connections touching both `a` and `b` (either direction), borrowed.
	/// Empty if none. Useful for listing/inspecting the links between two nodes.
	#[must_use]
	pub fn connections_between(&self, a: NodeId, b: NodeId) -> Vec<&Connection<'a>> {
		self.connections
			.iter()
			.filter(|c| {
				(c.from_node == a && c.to_node == b)
					|| (c.from_node == b && c.to_node == a)
			})
			.collect()
	}

	/// `true` if the graph changed since the last [`calculate`][Self::calculate]
	/// (any node/connection added or removed). Mutators do not re-run layout
	/// automatically, so a dirty graph means [`positions`] / [`split`] may be
	/// stale until you call `calculate`.
	///
	/// [`positions`]: Self::positions
	/// [`split`]: Self::split
	pub fn is_dirty(&self) -> bool {
		self.dirty
	}

	/// All node placements from the last [`calculate`][Self::calculate], keyed
	/// by [`NodeId`]. Each [`Rect`] is the node's **full frame** (border
	/// included) in **canvas coordinates** (origin top-left of the off-screen
	/// canvas, x growing leftwards — see [`split`] for the mirroring).
	///
	/// [`split`]: Self::split
	pub fn positions(&self) -> &Map<NodeId, Rect> {
		&self.placements
	}

	/// The full-frame [`Rect`] (border included, canvas coordinates) of a
	/// single node, or `None` if the node was not placed (unreachable / unknown
	/// / never calculated).
	pub fn node_rect(&self, id: NodeId) -> Option<Rect> {
		self.placements.get(&id).copied()
	}

	/// The node's full-frame [`Rect`] (border included) in **canvas** coordinates
	/// — i.e. where it lives in the off-screen canvas the stateful render blits,
	/// after the `Rtl`/`Btt` main-axis mirror. This is the counterpart of
	/// [`node_rect`][Self::node_rect] (which returns layout coords) for code that
	/// works in canvas space, e.g. feeding [`FlowState::ensure_visible`] or
	/// [`FlowState::center_on`] to keep a node in view. Returns `None` if the
	/// node isn't placed (or has a 0×0 placement).
	#[must_use]
	pub fn node_canvas_rect(&self, id: NodeId) -> Option<Rect> {
		let layout = self.placements.get(&id).copied()?;
		if layout.width == 0 || layout.height == 0 {
			return None;
		}
		let mut pos = layout;
		if self.direction.is_horizontal() {
			if self.direction.mirror_main_axis() {
				pos.x = (self.width as u16).saturating_sub(pos.right());
			}
		} else if self.direction.mirror_main_axis() {
			pos.y = (self.height as u16).saturating_sub(pos.bottom());
		}
		Some(pos)
	}

	/// Internal helper: borrow the [`NodeLayout`] for a node id, if present.
	fn node_layout(&self, id: NodeId) -> Option<&NodeLayout<'a>> {
		self.nodes.iter().find_map(|(nid, n)| (*nid == id).then_some(n))
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
		self.dirty = false;
		// Reset the connection router's accumulated state so repeated
		// `calculate()` calls are idempotent: without this, `conn_layout` would
		// keep appending to its connection list and leave stale `Edge::Connection`
		// marks in the edge field from the previous run, both slowing routing
		// down (the list grows each call) and polluting routing costs.
		self.conn_layout.reset();

		// Build the set of known node ids up front so the connection filter
		// below doesn't need to re-borrow `self` (which would conflict with
		// the `&mut self.diagnostics` push for invalid refs).
		let known_ids: Set<NodeId> = self.nodes.iter().map(|(id, _)| *id).collect();

		// filter out connections that reference unknown node ids. these
		// would otherwise panic on indexing later. we keep `self.connections`
		// untouched and only work off this local slice for the rest of
		// calculate().
		let mut bad_refs: Vec<Diagnostic> = Vec::new();
		let valid_conns: Vec<Connection<'a>> = self
			.connections
			.iter()
			.copied()
			.filter(|ea| {
				if !known_ids.contains(&ea.from_node) || !known_ids.contains(&ea.to_node)
				{
					log::warn!(
						"skipping connection: node id not present \
						 (from_node={}, to_node={})",
						ea.from_node,
						ea.to_node,
					);
					bad_refs.push(Diagnostic::InvalidConnectionRef {
						from_node: ea.from_node,
						to_node: ea.to_node,
					});
					false
				} else {
					true
				}
			})
			.collect();
		self.diagnostics.append(&mut bad_refs);

		// Placement phase: Auto/Pinned run the recursive layout from the root
		// nodes; Manual reads each node's position from `manual_positions` and
		// reports un-set nodes as UnplacedNode. All arms populate
		// `self.placements`; everything below (routing setup, conn_layout, canvas
		// render) consumes `placements` unchanged.
		//
		// `Auto` and `Pinned` share the same roots-walk; the only difference is
		// that `Pinned` first pre-populates `placements` with the pinned rects so
		// the walk treats them as immovable obstacles. In `Auto` the pre-populate
		// step is a no-op (the map is empty for node ids that exist), so the walk
		// runs against an empty placement set byte-for-byte as before.
		let run_auto_walk = match self.layout_mode {
			LayoutMode::Manual => {
				// Skip place_node entirely; place each node at its recorded
				// top-left (layout/canvas space). Nodes without an entry become
				// UnplacedNode diagnostics below (same as unreachable nodes in
				// Auto).
				for (id, layout) in &self.nodes {
					if let Some(&(x, y)) = self.manual_positions.get(id) {
						let rect = Rect::new(x, y, layout.size.0, layout.size.1);
						self.placements.insert(*id, rect);
					}
				}
				false
			}
			LayoutMode::Pinned => {
				// Pre-populate the pinned nodes as immovable anchors BEFORE the
				// walk. place_node treats any id already in `placements` as an
				// anchor (it neither overwrites nor nudges it, but still recurses
				// through it to reach its neighbors), so a pinned rect acts as
				// both a fixed coordinate and an obstacle the auto-placed nodes
				// flow around. Only ids that actually exist in `nodes` are pinned
				// (a position for a missing node is dead state, mirroring Manual).
				for (id, layout) in &self.nodes {
					if let Some(&(x, y)) = self.manual_positions.get(id) {
						let rect = Rect::new(x, y, layout.size.0, layout.size.1);
						self.placements.insert(*id, rect);
					}
				}
				true
			}
			LayoutMode::Auto => true,
		};

		if run_auto_walk {
			// find root nodes: every node id that is never a `from_node`.
			let mut roots: Set<_> = self.nodes.iter().map(|(id, _)| *id).collect();
			for ea_connection in valid_conns.iter() {
				roots.remove(&ea_connection.from_node);
			}

			// place them and their children (recursively). Precompute the
			// upstream adjacency once (O(connections)) so place_node/nudge
			// don't each re-scan + re-sort the whole connection list per
			// node.
			let upstream = build_upstream_index(&valid_conns);
			let mut main_chain = Vec::new();
			let mut visited = Set::new();
			for ea_root in roots {
				self.place_node(ea_root, 0, 0, &mut main_chain, &mut visited, &upstream);
				assert!(main_chain.is_empty());
			}
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
			// NOTE: canvas main axis is mirrored relative to layout coords for
			// Rtl/Btt (see FlowDirection). Compute each port's canvas point from
			// its layout rect + port index, parameterized by the flow direction.
			//
			// Horizontal (Ltr/Rtl): the main axis is x. The `from` (child) port
			// sits at the node's main-axis leading edge (layout x), the `to`
			// (parent) port one cell outside the trailing edge (layout right);
			// the cross axis (y) is top + port + 1.
			//
			// Vertical (Ttb/Btt): the main axis is y, cross axis is x. Ports run
			// along the node's width instead of its height, and the main-axis
			// mirror (if any) is applied to y.
			let dir = self.direction;
			let (a_point, b_point) = if dir.is_horizontal() {
				// defensively clamp port offsets to the node's inner height.
				let from_port = clamp_port(ea_conn.from_port.0 as usize, a_pos.height);
				let to_port = clamp_port(ea_conn.to_port.0 as usize, b_pos.height);
				let mw = self.width;
				// `from` (child) port at the node's main-axis leading edge
				// (layout left); `to` (parent) port one cell outside the
				// trailing edge (layout right). mirror_axis maps layout→canvas.
				let ax = mirror_axis(mw, a_pos.left() as usize, dir);
				let bx = mirror_axis(mw, b_pos.right() as usize, dir).saturating_sub(1);
				let a = (ax, a_pos.top() as usize + from_port + 1);
				let b = (bx, b_pos.top() as usize + to_port + 1);
				(a, b)
			} else {
				// vertical: ports index along the node's width (cross axis = x).
				// `from` (child) port at the node's top edge (layout top), `to`
				// (parent) port one cell outside the bottom edge (layout bottom).
				let from_port = clamp_port(ea_conn.from_port.0 as usize, a_pos.width);
				let to_port = clamp_port(ea_conn.to_port.0 as usize, b_pos.width);
				let mh = self.height;
				let ay = mirror_axis(mh, a_pos.top() as usize, dir);
				let by = mirror_axis(mh, b_pos.bottom() as usize, dir);
				let a = (a_pos.left() as usize + from_port + 1, ay);
				let b = (b_pos.left() as usize + to_port + 1, by);
				(a, b)
			};
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
			let vertical = !dir.is_horizontal();
			self.conn_layout.block_port(a_point, vertical);
			self.conn_layout.block_port(b_point, vertical);
		}
		for mut ea_placement in self.placements.values().cloned() {
			if self.direction.is_horizontal() && self.direction.mirror_main_axis() {
				ea_placement.x = (self.width as u16)
					.saturating_sub(ea_placement.x + ea_placement.width);
			} else if !self.direction.is_horizontal() && self.direction.mirror_main_axis()
			{
				ea_placement.y = (self.height as u16)
					.saturating_sub(ea_placement.y + ea_placement.height);
			}
			self.conn_layout.block_zone(ea_placement);
		}
		self.conn_layout.calculate(self.direction);

		// pull routing failures (RoutingFailed) up from the connection layout
		// into the graph-level diagnostics. `append` moves conn_layout's buffer
		// in wholesale and leaves it empty, which is fine — conn_layout's
		// diagnostics aren't read again after this point and are repopulated
		// on the next calculate().
		self.diagnostics.append(&mut self.conn_layout.diagnostics);

		// any node that never got a placement is unreachable (not on any root's
		// upstream chain — e.g. a pure cycle or an isolated node). record it.
		for (node_id, _) in &self.nodes {
			if !self.placements.contains_key(node_id) {
				log::warn!("unreachable node not placed (node={})", node_id);
				self.diagnostics.push(Diagnostic::UnplacedNode { node: *node_id });
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

	/// Place a node and recurse into its children.
	///
	/// `x` is the **main-axis** coordinate (the flow direction — children
	/// advance along it away from the root); `y` is the **cross-axis**
	/// coordinate (siblings stack along it). For horizontal flows (`Ltr`/`Rtl`)
	/// the main axis is x and the cross axis is y; for vertical flows it is
	/// reversed. The direction is read from `self.direction`.
	fn place_node(
		&mut self,
		idx_node: NodeId,
		x: u16,
		y: u16,
		main_chain: &mut Vec<NodeId>,
		visited: &mut Set<NodeId>,
		upstream: &UpstreamIndex<'a>,
	) {
		// cycle guard: if this node was already placed (reachable again through
		// a cycle), don't re-place or recurse. this is what prevents stack
		// overflows on root-reachable cycles. NOTE: this early-return only fires
		// for nodes placed *during* this walk; a node pre-populated by the
		// `Pinned` arm (an immovable anchor) is handled by the `is_anchor` branch
		// below, which still recurses through it.
		if !visited.insert(idx_node) {
			return;
		}

		// place the node
		let Some(node_layout) = self.node_layout(idx_node) else {
			// node id not present (defensive; calculate() already filtered).
			return;
		};
		let horiz = self.direction.is_horizontal();

		// `Pinned` mode pre-populates `placements` with the pinned rects before
		// the walk. A node already present here is an immovable anchor: adopt its
		// fixed rect as-is (do NOT overwrite, nudge, or run the intersection loop
		// on it), but still recurse into its neighbors so they're placed relative
		// to the anchor's far edge. In `Auto`/`Manual` the walk starts against an
		// empty `placements`, so this branch never fires there — the layout is
		// byte-for-byte identical to the pre-`Pinned` behavior.
		let is_anchor = self.placements.contains_key(&idx_node);
		let mut rect_me = if is_anchor {
			self.placements[&idx_node]
		} else {
			let size_me = node_layout.size;
			Rect { x, y, width: size_me.0, height: size_me.1 }
		};

		if !is_anchor {
			// nudge placement. if a node intersects with another node, its entire
			// main chain (largest subset of nodes including this one where every
			// node is the first child of its parent) should be moved along the cross
			// axis to not intersect.
			//
			// Repeat the for loop until it runs all the way through without any
			// intersections. Surely there's a more efficient way to do this.
			'outer: loop {
				for (_, ea_them) in self.placements.iter() {
					if rect_me.intersects(*ea_them) {
						if horiz {
							rect_me.y = rect_me.y.max(ea_them.bottom());
						} else {
							rect_me.x = rect_me.x.max(ea_them.right());
						}
						continue 'outer;
					}
				}
				break;
			}
			for ea_node in main_chain.iter() {
				// Pinned anchors in the main chain are immovable: skip them so a
				// sibling's cross-axis nudge never overwrites their fixed coord.
				// In `Auto`/`Manual` `manual_positions` is irrelevant to the walk,
				// so this guard is inert there.
				if self.manual_positions.contains_key(ea_node) {
					continue;
				}
				if horiz {
					let y = self.placements[ea_node].y.max(rect_me.y);
					self.placements.get_mut(ea_node).unwrap().y = y;
				} else {
					let x = self.placements[ea_node].x.max(rect_me.x);
					self.placements.get_mut(ea_node).unwrap().x = x;
				}
			}
			self.placements.insert(idx_node, rect_me);
		}

		// find children and order them.
		// Siblings stack along the cross axis; initialize the cross-axis cursor
		// from this node's own cross-axis coordinate (NOT from a function param,
		// since the param mapping to main/cross flips with direction).
		let mut cross = if horiz { rect_me.y } else { rect_me.x };
		let cur_main = if horiz { rect_me.x } else { rect_me.y };
		let main_extent = if horiz { rect_me.width } else { rect_me.height };
		main_chain.push(idx_node);
		for ea_child in upstream.of(idx_node) {
			// A child already in `placements` is either (a) a node reached again
			// through a cycle — nudge it along the main axis — or (b) a pinned
			// anchor (pre-populated, in `manual_positions`) that must NOT be
			// nudged but still needs its neighbors walked. Route (b) through
			// place_node's `is_anchor` branch, which adopts the fixed rect and
			// recurses; in `Auto`/`Manual` `manual_positions` is irrelevant to
			// the walk so (b) never triggers and the original (a) path is
			// byte-for-byte unchanged.
			let child_is_pinned = self.manual_positions.contains_key(&ea_child.from_node);
			if self.placements.contains_key(&ea_child.from_node) && !child_is_pinned {
				// nudge it (if necessary). a fresh in_progress set per top-level
				// nudge tracks the recursion stack to break cycles without
				// blocking legitimate re-nudges. pass the new main-axis coordinate.
				let mut in_progress = Set::new();
				let new_main = cur_main + main_extent + MARGIN;
				self.nudge(ea_child.from_node, new_main, &mut in_progress, upstream);
			} else {
				// place it. child advances along the main axis; siblings stack
				// along the cross axis. place_node takes (x, y) where for
				// horizontal x=main,y=cross and for vertical x=cross,y=main.
				let child_main = cur_main + main_extent + MARGIN;
				let (px, py) =
					if horiz { (child_main, cross) } else { (cross, child_main) };
				self.place_node(
					ea_child.from_node,
					px,
					py,
					main_chain,
					visited,
					upstream,
				);
				main_chain.clear();
				// child may not have been placed (e.g. it's part of a cycle);
				// only advance the cross axis if it actually got a rect.
				if let Some(child_rect) = self.placements.get(&ea_child.from_node) {
					if horiz {
						cross += child_rect.height;
					} else {
						cross += child_rect.width;
					}
				}
			}
		}
		main_chain.pop();
	}

	/// Push a node (and its subtree) further along the main axis if its
	/// current main-axis coordinate is less than `main`. `main` is the
	/// main-axis coordinate (x for horizontal, y for vertical).
	fn nudge(
		&mut self,
		idx_node: NodeId,
		main: u16,
		in_progress: &mut Set<NodeId>,
		upstream: &UpstreamIndex<'a>,
	) {
		// Pinned anchors are immovable: never move them or their subtree along
		// the main axis. This guard is what keeps a pinned rect at its fixed
		// coordinate even when a neighbor's main-chain nudge would otherwise
		// push it. In `Auto`/`Manual` `manual_positions` is either empty or
		// irrelevant to the walk, so this branch is inert there.
		if self.manual_positions.contains_key(&idx_node) {
			return;
		}
		// cycle guard: break only if this node is already on the current
		// recursion stack (i.e. we're inside a cycle). a node legitimately
		// nudged twice through different paths must still be processed, so we
		// use a per-recursion-stack set, not a global visited set.
		if !in_progress.insert(idx_node) {
			return;
		}
		let rect_me = self.placements[&idx_node];
		let horiz = self.direction.is_horizontal();
		let cur_main = if horiz { rect_me.x } else { rect_me.y };
		let my_extent = if horiz { rect_me.width } else { rect_me.height };
		if cur_main < main {
			if horiz {
				self.placements.get_mut(&idx_node).unwrap().x = main;
			} else {
				self.placements.get_mut(&idx_node).unwrap().y = main;
			}
			for ea_child in upstream.of(idx_node) {
				// the child must already be placed for nudging to make sense;
				// skip defensively if not.
				if self.placements.contains_key(&ea_child.from_node) {
					self.nudge(
						ea_child.from_node,
						main + my_extent + MARGIN,
						in_progress,
						upstream,
					);
				}
			}
		}
		in_progress.remove(&idx_node);
	}

	pub fn split(&self, area: Rect) -> Vec<Rect> {
		self.split_named(area).into_iter().map(|(_, r)| r).collect()
	}

	/// Like [`split`][Self::split], but each entry is tagged with its [`NodeId`].
	/// The order matches [`Self::split`] (nodes in render order) and the rects
	/// are the same content rects (border removed, mirrored to screen coordinates
	/// within `area`).
	///
	/// Useful when you need to map a placed rect back to the node it belongs to
	/// (e.g. hit testing).
	pub fn split_named(&self, area: Rect) -> Vec<(NodeId, Rect)> {
		self.nodes
			.iter()
			.map(|(id, _)| {
				let r = self
					.placements
					.get(id)
					.map(|pos| {
						if pos.right() > area.width || pos.bottom() > area.height {
							return Rect { x: 0, y: 0, width: 0, height: 0 };
						}
						let mut pos = *pos;
						if self.direction.is_horizontal() {
							if self.direction.mirror_main_axis() {
								pos.x = area.width - pos.right() + area.x;
							} else {
								pos.x += area.x;
							}
							pos.y += area.y;
						} else {
							pos.x += area.x;
							if self.direction.mirror_main_axis() {
								pos.y = area.height - pos.bottom() + area.y;
							} else {
								pos.y += area.y;
							}
						}
						pos.inner(Margin { horizontal: 1, vertical: 1 })
					})
					.unwrap_or_default();
				(*id, r)
			})
			.collect()
	}

	/// Hit-test a **canvas** coordinate against the placed nodes and return the
	/// [`NodeId`] of the node whose full frame (border included) contains
	/// `(x, y)`, or `None` if the point falls in empty space or on a node that
	/// was never placed.
	///
	/// `(x, y)` is a **canvas** coordinate: for a screen point under a panned
	/// view, the caller pre-translates it as `canvas = screen - area.origin +
	/// state.view_offset`. The hit area is each node's **bordered** rect (a
	/// point on the border counts as a hit), mirrored about the canvas size for
	/// `Rtl`/`Btt` — the same canvas-absolute space the stateful render blits, so
	/// a hit always corresponds to a visible border cell.
	///
	/// `area` is kept in the signature for stability but is not used by the hit
	/// math (the point is already in canvas coords). Must be called **after**
	/// [`calculate`][Self::calculate]; before that, [`positions`][Self::positions]
	/// is empty and `hit_test` always returns `None`. Nodes never overlap in
	/// canvas space, so at most one node can contain a given point; the first
	/// match (in render order) is returned.
	pub fn hit_test(&self, area: Rect, x: u16, y: u16) -> Option<NodeId> {
		let _ = area;
		for (id, canvas_rect) in self.placements.iter() {
			// A 0×0 placement means the node never fit / was never placed —
			// it has no on-screen extent to hit.
			if canvas_rect.width == 0 || canvas_rect.height == 0 {
				continue;
			}
			// Skip nodes whose layout placement exceeds the canvas (defensive —
			// placements are normally within bounds).
			if canvas_rect.right() > self.width as u16
				|| canvas_rect.bottom() > self.height as u16
			{
				continue;
			}
			// Mirror the layout placement into canvas coords (same transform as
			// the off-screen canvas), WITHOUT `.inner(Margin{1,1})` so the
			// border is part of the hit rect. (x,y) are canvas coords too.
			let mut pos = *canvas_rect;
			if self.direction.is_horizontal() {
				if self.direction.mirror_main_axis() {
					pos.x = (self.width as u16).saturating_sub(pos.right());
				}
			} else if self.direction.mirror_main_axis() {
				pos.y = (self.height as u16).saturating_sub(pos.bottom());
			}
			if pos.contains(Position { x, y }) {
				return Some(*id);
			}
		}
		None
	}

	/// Like [`hit_test`][Self::hit_test], but if the canvas point `(x, y)` lands
	/// on a node's PORT cell, return `(node, port, is_input)` identifying which
	/// port. `is_input == true` means the `to` (in) port, `false` the `from`
	/// (out) port. Returns `None` for empty space, node interiors, or border
	/// cells that aren't ports. `(x, y)` are CANVAS coordinates (same contract as
	/// [`hit_test`][Self::hit_test]).
	///
	/// Must be called after [`calculate`][Self::calculate]; before that nothing
	/// is placed and this always returns `None`. Port cells are computed the same
	/// way the internal `render_to` pass draws them, so a hit always corresponds
	/// to a visible port glyph (the first match in render order is returned).
	///
	/// Internally this scans the rasterized canvas produced by `render_to`.
	#[must_use]
	pub fn hit_test_port(
		&self,
		area: Rect,
		x: u16,
		y: u16,
	) -> Option<(NodeId, PortId, bool)> {
		let _ = area;
		let horiz = self.direction.is_horizontal();
		let mirror = self.direction.mirror_main_axis();
		// Mirror the layout placement into canvas coords the same way hit_test /
		// render_to do (Rtl mirrors x about the canvas width, Btt mirrors y about
		// the canvas height; Ltr/Ttb are unchanged), then check each port cell
		// rendered on that canvas rect. The port cell math mirrors render_to
		// exactly (it is the source of truth for where ports are drawn).
		let canvas_w = self.width as u16;
		let canvas_h = self.height as u16;
		for (id, canvas_rect) in self.placements.iter() {
			if canvas_rect.width == 0 || canvas_rect.height == 0 {
				continue;
			}
			if canvas_rect.right() > canvas_w || canvas_rect.bottom() > canvas_h {
				continue;
			}
			let mut pos = *canvas_rect;
			if horiz {
				if mirror {
					pos.x = canvas_w.saturating_sub(pos.right());
				}
			} else if mirror {
				pos.y = canvas_h.saturating_sub(pos.bottom());
			}
			// Iterate every connection that references this node and compute its
			// rendered port cell. The port index is clamped to the node's inner
			// extent the same way render_to clamps it (clamp_port).
			for c in &self.connections {
				if c.to_node == *id {
					let port = clamp_port(c.to_port.0 as usize, pos.height) as u16;
					let (px, py) = if horiz {
						(pos.left(), pos.top() + port + 1)
					} else {
						(pos.left() + port + 1, pos.bottom() - 1)
					};
					if px == x && py == y {
						return Some((*id, c.to_port, true));
					}
				}
				if c.from_node == *id {
					let port = clamp_port(c.from_port.0 as usize, pos.height) as u16;
					let (px, py) = if horiz {
						(pos.right() - 1, pos.top() + port + 1)
					} else {
						(pos.left() + port + 1, pos.top())
					};
					if px == x && py == y {
						return Some((*id, c.from_port, false));
					}
				}
			}
		}
		None
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
	#[deprecated(
		since = "0.1.0",
		note = "use `split_stateful(area, &FlowState)` with a FlowState whose `view_offset` is set"
	)]
	#[allow(deprecated)]
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

	/// Like [`split_named`][Self::split_named], but each content rect is
	/// translated by `state.view_offset` and clipped to `area` — i.e. it returns
	/// the screen-coordinate content rects for a scrolled/panned view driven by
	/// a [`FlowState`].
	///
	/// This is the stateful-path counterpart of
	/// [`split_viewport`][Self::split_viewport]: it yields `(NodeId, Rect)`
	/// pairs (so the caller can route content to the right node), and the rect
	/// math is identical — each node's canvas-rect (mirrored, inner border
	/// removed) is shifted by `-view_offset + area.origin` and clipped to `area`.
	/// A node fully scrolled off-screen yields a 0×0 rect (render it only when
	/// `width > 0 && height > 0`).
	///
	/// Typical per-frame usage with the stateful render path:
	/// ```ignore
	/// let zones = graph.split_stateful(view_area, &flow_state);
	/// for (id, z) in &zones {
	///     if z.width > 0 && z.height > 0 {
	///         f.render_widget(my_content_for(id), *z);
	///     }
	/// }
	/// f.render_stateful_widget(graph, view_area, &mut flow_state);
	/// ```
	pub fn split_stateful(
		&self,
		area: Rect,
		state: &crate::FlowState,
	) -> Vec<(NodeId, Rect)> {
		let canvas_rect = Rect {
			x: 0,
			y: 0,
			width: self.width as u16,
			height: self.height as u16,
		};
		let (ox, oy) = state.view_offset;
		self.split_named(canvas_rect)
			.into_iter()
			.map(|(id, z)| {
				// A node whose entire rect sits above/left of the viewport is
				// fully scrolled off and must become 0×0 (see split_viewport).
				if z.right() <= ox || z.bottom() <= oy {
					return (id, Rect::default());
				}
				let mut z = z;
				z.x = z.x.saturating_sub(ox).saturating_add(area.x);
				z.y = z.y.saturating_sub(oy).saturating_add(area.y);
				(id, z.intersection(area))
			})
			.collect()
	}

	/// Translate a node's layout placement (border included) into the on-screen
	/// rect its border is actually drawn at, so the stateful highlight overlay
	/// lands on the right cells. Returns `None` if the node isn't placed, has a
	/// 0×0 rect, or ends up fully clipped out of `area`.
	///
	/// Borders always live in **canvas-absolute** coordinates: the off-screen
	/// canvas (rendered once during [`calculate`][Self::calculate]) mirrors the
	/// layout placement about the canvas size for `Rtl`/`Btt`, and the stateful
	/// render path blits a window of it at every offset. So this mirrors the
	/// placement into canvas coords the same way, then translates by
	/// `-view_offset + area.origin`. Non-mirrored directions (`Ltr`/`Ttb`) map
	/// straight through. This is consistent at every pan offset, so the
	/// highlight tracks the border whether or not the view is panned.
	fn node_screen_rect(
		&self,
		id: NodeId,
		area: Rect,
		view_offset: (u16, u16),
	) -> Option<Rect> {
		let layout = self.placements.get(&id).copied()?;
		if layout.width == 0 || layout.height == 0 {
			return None;
		}
		let (ox, oy) = view_offset;
		let horiz = self.direction.is_horizontal();
		let mirror = self.direction.mirror_main_axis();

		let mut pos = layout;
		// Mirror the layout placement into canvas coords (the canvas holds the
		// mirrored rendering). Rtl mirrors x about the canvas width; Btt
		// mirrors y about the canvas height; Ltr/Ttb are unchanged.
		if horiz {
			if mirror {
				pos.x = (self.width as u16).saturating_sub(pos.right());
			}
		} else if mirror {
			pos.y = (self.height as u16).saturating_sub(pos.bottom());
		}
		// fully scrolled off the top/left edge -> not visible.
		if pos.right() <= ox || pos.bottom() <= oy {
			return None;
		}
		// canvas coords -> screen: subtract the pan, add the area origin.
		pos.x = pos.x.saturating_sub(ox).saturating_add(area.x);
		pos.y = pos.y.saturating_sub(oy).saturating_add(area.y);

		// clip to area; if no overlap, not visible.
		let clipped = pos.intersection(area);
		if clipped.width == 0 || clipped.height == 0 {
			return None;
		}
		Some(clipped)
	}

	/// Recolor the **border** cells of a node's on-screen rect with `style`'s
	/// foreground/add-modifier, preserving each cell's existing symbol (so the
	/// port glyphs ├/┬/etc. that the canvas blit already placed on the border
	/// survive — only their color changes). Only the perimeter cells are
	/// touched; the node's inner content is left untouched.
	///
	/// This is the highlight mechanism used by the stateful render path: after
	/// the (possibly scrolled) canvas is blitted, the selection/hover node's
	/// border is recolored in place without re-running `Block::render` (which
	/// would clobber the port symbols).
	fn highlight_border(&self, buf: &mut Buffer, screen_rect: Rect, style: Style) {
		let r = screen_rect;
		// perimeter: top row, bottom row, left col, right col — clamped to the
		// rect itself (a 1-wide/1-tall rect degenerates to a single row/col,
		// which is correct: its only cells are border cells).
		for x in r.left()..r.right() {
			if let Some(cell) = buf.cell_mut(Position::new(x, r.top())) {
				merge_style(cell, style);
			}
			if r.height > 1
				&& let Some(cell) = buf.cell_mut(Position::new(x, r.bottom() - 1))
			{
				merge_style(cell, style);
			}
		}
		for y in r.top()..r.bottom() {
			if let Some(cell) = buf.cell_mut(Position::new(r.left(), y)) {
				merge_style(cell, style);
			}
			if r.width > 1
				&& let Some(cell) = buf.cell_mut(Position::new(r.right() - 1, y))
			{
				merge_style(cell, style);
			}
		}
	}

	// ----- text export ----------------------------------------------------

	/// Dump the rendered graph's skeleton (borders / ports / connections — no
	/// node *content*) as a `String` of box-drawing characters, one canvas row
	/// per line joined by `'\n'`.
	///
	/// Walks the internal off-screen canvas in row-major order; each cell
	/// contributes its [`symbol`](ratatui::buffer::Cell::symbol) (an empty cell
	/// is `' '`).
	/// Trailing spaces on every row are **kept** (full-grid fidelity) — no
	/// trimming — so the output width is constant per row and reconstructing the
	/// grid is unambiguous.
	///
	/// Must be called **after** [`calculate`][Self::calculate]; before that the
	/// off-screen canvas is blank (every cell is a space), so the result is
	/// `height` lines of `width` spaces (defensive, not an error). The output is
	/// a pure read: neither `self.canvas` nor any other state is mutated.
	///
	/// Each cell symbol is a single grapheme for box-drawing glyphs, but
	/// [`ratatui::buffer::Cell::symbol`] returns a `&str` to allow for
	/// multi-codepoint clusters; we copy it verbatim, so combining marks (if any
	/// ever appear) survive.
	pub fn to_ascii(&self) -> String {
		let mut rows: Vec<String> = Vec::with_capacity(self.height);
		for y in 0..self.height {
			let mut row = String::with_capacity(self.width);
			for x in 0..self.width {
				let sym = self
					.canvas
					.cell(Position::new(x as u16, y as u16))
					.map(ratatui::buffer::Cell::symbol)
					.unwrap_or(" ");
				row.push_str(sym);
			}
			rows.push(row);
		}
		rows.join("\n")
	}

	/// Like [`to_ascii`][Self::to_ascii], but overlays each node's `content`
	/// (multi-line text) into its interior, on top of the skeleton.
	///
	/// For every placed node, `content(id)` is called once; if it returns
	/// `Some(text)`, each line of `text` is written into the node's interior
	/// (the `(width-2, height-2)` area one cell inside the border), starting at
	/// the interior's top-left. Lines are truncated by **display width** (via
	/// `unicode-width`, matching [`NodeLayout::from_content`]) to the interior
	/// width, so wide (CJK) and zero-width characters measure correctly. Lines
	/// past the interior height are dropped. A line that is empty after
	/// truncation is skipped (the skeleton row underneath shows through). When
	/// `content(id)` returns `None`, the node's interior is left blank (skeleton
	/// only).
	///
	/// `'c` is the lifetime of the borrowed content strings returned by
	/// `content`; it is independent of the graph's own `'a` lifetime (the
	/// strings are copied into the output and not retained).
	///
	/// Only cells the content actually covers are overwritten; the rest of each
	/// interior row keeps whatever the skeleton drew (port names, etc.). Must be
	/// called after [`calculate`][Self::calculate]; before that the placements
	/// are empty and the result equals [`to_ascii`][Self::to_ascii] (all spaces).
	/// Like `to_ascii`, this is a pure read — no state is mutated.
	///
	/// [`NodeLayout::from_content`]: crate::NodeLayout::from_content
	pub fn to_ascii_with<'c>(
		&self,
		content: impl Fn(NodeId) -> Option<&'c str>,
	) -> String {
		// Start from the skeleton rows so borders/ports/connections survive
		// untouched; we only stamp content into interior cells.
		let mut rows: Vec<String> = Vec::with_capacity(self.height);
		for y in 0..self.height {
			let mut row = String::with_capacity(self.width);
			for x in 0..self.width {
				let sym = self
					.canvas
					.cell(Position::new(x as u16, y as u16))
					.map(ratatui::buffer::Cell::symbol)
					.unwrap_or(" ");
				row.push_str(sym);
			}
			rows.push(row);
		}

		// Stamp each node's content into its interior. We use the CANVAS-space
		// rect (`node_canvas_rect`, direction-mirrored), NOT the raw placement —
		// the skeleton borders are drawn in canvas space (see `render_to`), so
		// content must be placed there too, or it lands outside its frame under
		// Rtl/Btt. Interior is one cell in on every side: x in
		// [rect.x+1, rect.right-1), y in [rect.y+1, rect.bottom-1).
		for (id, _) in &self.nodes {
			let Some(rect) = self.node_canvas_rect(*id) else {
				continue;
			};
			let Some(text) = content(*id) else { continue };
			// Interior content width in display cells (>=0). A node smaller than
			// 2x2 has no interior; saturating_sub yields 0 and every line gets
			// truncated to nothing, which is correct.
			let inner_w = rect.width.saturating_sub(2) as usize;
			// Last interior row index (exclusive) in canvas coords. Rows are
			// written starting at rect.y+1; the last valid interior row is
			// rect.bottom()-1 (exclusive), i.e. rect.y + rect.height - 1.
			let inner_bottom =
				(rect.y as usize).saturating_add(rect.height as usize).saturating_sub(1);
			for (i, line) in text.lines().enumerate() {
				let row_idx = (rect.y as usize).saturating_add(1).saturating_add(i);
				// Out of interior height or off the canvas: stop (subsequent
				// lines would only be further out).
				if row_idx >= inner_bottom || row_idx >= self.height {
					break;
				}
				if line.is_empty() {
					continue;
				}
				let stamped = truncate_to_width(line, inner_w);
				if stamped.is_empty() {
					continue;
				}
				// Overwrite the interior slice [rect.x+1, rect.x+1+stamped_len)
				// with the truncated line. Anything outside that range on this
				// row is preserved from the skeleton.
				let row_str = rows.get_mut(row_idx).expect("row within height");
				let start = (rect.x as usize).saturating_add(1);
				let end = start + stamped.chars().count();
				// Build a char-indexed replacement: replace [start, end) with the
				// stamped characters, keeping the prefix and suffix intact. We
				// operate on chars (not bytes) since prior cells are single
				// box-drawing chars but the row is a String.
				let prefix: String = row_str.chars().take(start).collect();
				let suffix: String = row_str.chars().skip(end).collect();
				*row_str = format!("{prefix}{stamped}{suffix}");
			}
		}
		rows.join("\n")
	}
}

/// Truncate `s` to at most `max_width` display cells, using `unicode-width` so
/// wide (CJK) and zero-width characters measure the same way
/// [`NodeLayout::from_content`] measures them. Stops at the first character
/// that would push the accumulated width over `max_width`. Combining marks
/// (zero width) stay attached to the preceding base char as long as the base
/// fits.
///
/// Returns the truncated string (owned, since the cut point is mid-string for
/// wide chars). Empty when `max_width == 0` or `s` is empty.
///
/// [`NodeLayout::from_content`]: crate::NodeLayout::from_content
fn truncate_to_width(s: &str, max_width: usize) -> String {
	use unicode_width::UnicodeWidthChar;
	let mut out = String::new();
	let mut width = 0usize;
	for ch in s.chars() {
		let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
		if width + cw > max_width {
			break;
		}
		width += cw;
		out.push(ch);
	}
	out
}

/// Overlay `style` onto an existing cell without changing the cell's symbol.
/// Used by the stateful highlight to recolor a node's border while preserving
/// the port glyphs already on it.
///
/// `Style::patch` merges fg/bg with `or` and modifiers with `insert` (set
/// union), which is exactly the "keep existing, add new" semantics we want.
fn merge_style(cell: &mut ratatui::buffer::Cell, style: Style) {
	let merged = cell.style().patch(style);
	cell.set_style(merged);
}

/// Scroll position into a [`NodeGraph`]'s canvas.
///
/// `offset` is the (x, y) coordinate of the viewport's top-left corner within
/// the off-screen canvas. (0, 0) shows the top-left of the graph; increasing x
/// scrolls right, increasing y scrolls down.
///
/// Pass it to [`NodeGraph::split_viewport`] (for node content rects) and
/// [`NodeGraphView`] (for borders/connections).
#[deprecated(
	since = "0.1.0",
	note = "use `FlowState` (its `view_offset` field replaces `Viewport.offset`)"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Viewport {
	/// Top-left corner of the viewport in canvas coordinates.
	pub offset: (u16, u16),
}

#[allow(deprecated)]
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

#[allow(deprecated)]
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
#[deprecated(
	since = "0.1.0",
	note = "use `NodeGraph`'s `StatefulWidget` impl with `FlowState` (set `view_offset` for panning)"
)]
pub struct NodeGraphView<'a> {
	graph: &'a NodeGraph<'a>,
	#[allow(deprecated)]
	viewport: Viewport,
}

#[allow(deprecated)]
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

#[allow(deprecated)]
impl Widget for NodeGraphView<'_> {
	fn render(self, area: Rect, buf: &mut Buffer) {
		let canvas = self.graph.canvas();
		let (ox, oy) = self.viewport.offset;
		blit_canvas(canvas, area, buf, (ox, oy));
	}
}

/// Blit the visible window of `canvas` onto `area` of `buf`, cell by cell.
///
/// `offset` is the top-left corner of the visible window inside `canvas`. For
/// each screen cell `(vx, vy)` in `area`, the canvas cell at
/// `(offset.x + vx, offset.y + vy)` is looked up; if it's non-blank, its symbol
/// and style are copied into the matching `buf` cell. Blank canvas cells (node
/// content / background) are left untouched so caller-rendered content shows
/// through.
///
/// Shared between [`NodeGraphView::render`] (the legacy stateless viewport
/// widget) and [`NodeGraph`]'s stateful render path so both blit identically.
pub(crate) fn blit_canvas(
	canvas: &Buffer,
	area: Rect,
	buf: &mut Buffer,
	offset: (u16, u16),
) {
	let (ox, oy) = offset;
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

/// Per-node upstream connection lists (the children feeding INTO each node),
/// precomputed once from the filtered `valid_conns` and reused across the whole
/// `place_node` / `nudge` recursion. Replaces the per-call `get_upstream` full
/// scan + sort (O(connections) each, called once per node).
///
/// Ordering matches the old `get_upstream` exactly: sorted by `to_port` with a
/// stable sort over the connection iteration order.
struct UpstreamIndex<'a>(Map<NodeId, Vec<Connection<'a>>>);

impl<'a> UpstreamIndex<'a> {
	/// Connections feeding INTO `node` (`to_node == node`), sorted by `to_port`.
	/// Empty slice if none — matches `get_upstream`'s empty `Vec`.
	fn of(&self, node: NodeId) -> &[Connection<'a>] {
		self.0.get(&node).map(Vec::as_slice).unwrap_or(&[])
	}
}

fn build_upstream_index<'a>(conns: &[Connection<'a>]) -> UpstreamIndex<'a> {
	let mut m: Map<NodeId, Vec<Connection<'a>>> = Map::new();
	for c in conns {
		m.entry(c.to_node).or_default().push(*c);
	}
	for v in m.values_mut() {
		v.sort_by_key(|c| c.to_port);
	}
	UpstreamIndex(m)
}

fn get_upstream<'a>(conns: &[Connection<'a>], idx_node: NodeId) -> Vec<Connection<'a>> {
	// find children and order them
	let mut upstream: Vec<_> =
		conns.iter().filter(|ea| ea.to_node == idx_node).copied().collect();
	upstream.sort_by_key(|a| a.to_port);
	upstream
}

fn get_downstream<'a>(conns: &[Connection<'a>], idx_node: NodeId) -> Vec<Connection<'a>> {
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

/// Direction a port name grows from its anchor cell (Step 9).
enum PortNameDir {
	/// First char at the anchor cell, subsequent chars to the right (used for
	/// **in** ports on the left/top edge, so the name grows toward the node
	/// interior).
	Right,
	/// First char at the anchor cell, subsequent chars to the left (used for
	/// **out** ports on the right/bottom edge, so the name grows toward the node
	/// interior). The anchor cell is thus the inner-most cell and holds the
	/// name's first character.
	Left,
}

/// Draw a port's display name horizontally, anchored at `(x, y)` (one cell
/// *inside* the node from its port symbol) and growing in `dir`. The name is
/// truncated so it never leaves the node's inner content area (`pos` is the
/// node's full frame, so the inner row is `pos.left()+1 ..= pos.right()-2`):
/// the anchor cell always holds the first character (so the "inner cell next to
/// the port symbol" is the name's leading char), and remaining characters extend
/// inward. Bounds against `area` are also checked (render_to may have clipped
/// the node). The name is drawn in **bold**.
///
/// Pure overlay: only called when a name exists, so no-name nodes render
/// identically to pre-Step-9.
fn write_port_name(
	buf: &mut Buffer,
	area: Rect,
	pos: Rect,
	x: u16,
	y: u16,
	dir: PortNameDir,
	name: &str,
) {
	let style = Style::default().add_modifier(Modifier::BOLD);
	let inner_left = pos.left() + 1;
	let inner_right = pos.right().saturating_sub(2);
	match dir {
		PortNameDir::Right => {
			let mut cx = x;
			for ch in name.chars() {
				if cx > inner_right {
					break;
				}
				if cx >= area.width || y >= area.height {
					break;
				}
				if let Some(cell) = buf.cell_mut(Position::new(cx, y)) {
					cell.set_symbol(ch.to_string().as_str()).set_style(style);
				}
				cx = cx.saturating_add(1);
			}
		}
		PortNameDir::Left => {
			let mut cx = x;
			for ch in name.chars() {
				if cx < inner_left {
					break;
				}
				if cx >= area.width || y >= area.height {
					break;
				}
				if let Some(cell) = buf.cell_mut(Position::new(cx, y)) {
					cell.set_symbol(ch.to_string().as_str()).set_style(style);
				}
				cx = cx.saturating_sub(1);
			}
		}
	}
}

/// Map a layout-coordinate main-axis value to its canvas-coordinate value,
/// mirroring about `size` when the direction's main axis is mirrored
/// (`Rtl`/`Btt`). `size` is the canvas extent on the main axis
/// (`self.width` for horizontal, `self.height` for vertical).
///
/// For non-mirrored directions (`Ltr`/`Ttb`) the value is unchanged.
fn mirror_axis(size: usize, coord: usize, dir: FlowDirection) -> usize {
	if dir.mirror_main_axis() { size.saturating_sub(coord) } else { coord }
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

		let vertical = !self.direction.is_horizontal();
		let mirror = self.direction.mirror_main_axis();

		// draw nodes
		'node: for (idx_node, ea_node) in &self.nodes {
			let idx_node = *idx_node;
			if let Some(mut pos) = self.placements.get(&idx_node).copied() {
				if pos.right() > area.width || pos.bottom() > area.height {
					continue 'node;
				}
				// draw box — mirror the main axis to screen coordinates when the
				// direction mirrors (Rtl mirrors x, Btt mirrors y); otherwise just
				// translate by the area origin.
				if self.direction.is_horizontal() {
					if mirror {
						pos.x = area.left() + area.width - pos.right();
					} else {
						pos.x += area.left();
					}
					pos.y += area.top();
				} else {
					pos.x += area.left();
					if mirror {
						pos.y = area.top() + area.height - pos.bottom();
					} else {
						pos.y += area.top();
					}
				}
				let block = ea_node.block();
				block.render(pos, buf);
				// For horizontal flows the input (to_node) port is on the LEFT edge
				// and the output (from_node) port on the RIGHT edge. For vertical flows
				// the input port is on the BOTTOM edge (faces the child below) and the
				// output port on the TOP edge (faces the parent above) — i.e. swapped
				// relative to horizontal, because the parent sits on the opposite side.
				// draw input ports (upstream / to_node)
				for ea_conn in get_upstream(&self.connections, idx_node) {
					let to_port =
						clamp_port(ea_conn.to_port.0 as usize, pos.height) as u16;
					// draw connection alias
					if let Some(alias_char) = self.conn_layout.alias_connections.get(&(
						true,
						idx_node,
						ea_conn.to_port,
					)) {
						if !vertical {
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
						} else {
							// input alias one cell below the bottom edge
							let x = pos.left() + to_port + 1;
							if x < area.width
								&& let Some(cell) =
									buf.cell_mut(Position::new(x, pos.bottom()))
							{
								cell.set_symbol(alias_char).set_style(
									Style::default()
										.add_modifier(Modifier::BOLD)
										.bg(Color::Red),
								);
							}
						}
					}

					// draw port — the `to` (in) port. When `show_arrows` is on
					// (the default), this is a direction arrow pointing in the
					// flow direction (toward the inside of the `to` node);
					// otherwise it falls back to the in-port connection glyph
					// (`┤`/`┴` family) for Step-6-equivalent rendering.
					let in_symbol = if self.show_arrows {
						arrow_symbol(self.direction, ea_conn.line_type())
					} else {
						conn_symbol(
							true,
							ea_node.border_type(),
							ea_conn.line_type(),
							vertical,
						)
					};
					if !vertical {
						if let Some(cell) = buf
							.cell_mut(Position::new(pos.left(), pos.top() + to_port + 1))
						{
							cell.set_symbol(in_symbol);
						}
						// Step 9: port display name — drawn one cell inside the
						// left edge (toward the node interior), truncated to the
						// node's content area so it never crosses the far border.
						if let Some(name) = ea_node.port_name(ea_conn.to_port) {
							let y = pos.top() + to_port + 1;
							let anchor_x = pos.left() + 1;
							write_port_name(
								buf,
								area,
								pos,
								anchor_x,
								y,
								PortNameDir::Right,
								name,
							);
						}
					} else {
						// input port on the bottom edge, offset along width
						if let Some(cell) = buf.cell_mut(Position::new(
							pos.left() + to_port + 1,
							pos.bottom() - 1,
						)) {
							cell.set_symbol(in_symbol);
						}
						// Step 9: port name — one cell above the bottom edge
						// (into the node interior), written horizontally from the
						// port's x, truncated to the content area.
						if let Some(name) = ea_node.port_name(ea_conn.to_port) {
							let x = pos.left() + to_port + 1;
							let y = pos.bottom().saturating_sub(2);
							write_port_name(
								buf,
								area,
								pos,
								x,
								y,
								PortNameDir::Right,
								name,
							);
						}
					}
				}
				// draw output ports (downstream / from_node)
				for ea_conn in get_downstream(&self.connections, idx_node) {
					let from_port =
						clamp_port(ea_conn.from_port.0 as usize, pos.height) as u16;
					// draw connection alias
					if let Some(alias_char) = self.conn_layout.alias_connections.get(&(
						false,
						idx_node,
						ea_conn.from_port,
					)) {
						if !vertical {
							if let Some(cell) = buf.cell_mut(Position::new(
								pos.right(),
								pos.top() + from_port + 1,
							)) {
								cell.set_symbol(alias_char).set_style(
									Style::default()
										.add_modifier(Modifier::BOLD)
										.bg(Color::Red),
								);
							}
						} else {
							// output alias one cell above the top edge
							if pos.top() > 0
								&& let Some(cell) = buf.cell_mut(Position::new(
									pos.left() + from_port + 1,
									pos.top() - 1,
								)) {
								cell.set_symbol(alias_char).set_style(
									Style::default()
										.add_modifier(Modifier::BOLD)
										.bg(Color::Red),
								);
							}
						}
					}

					// draw port
					if !vertical {
						if let Some(cell) = buf.cell_mut(Position::new(
							pos.right() - 1,
							pos.top() + from_port + 1,
						)) {
							cell.set_symbol(conn_symbol(
								false,
								ea_node.border_type(),
								ea_conn.line_type(),
								false,
							));
						}
						// Step 9: out-port display name — one cell inside the
						// right edge (toward the node interior). First char on
						// the inner cell, remaining chars extend leftward.
						if let Some(name) = ea_node.port_name(ea_conn.from_port) {
							let y = pos.top() + from_port + 1;
							let anchor_x = pos.right().saturating_sub(2);
							write_port_name(
								buf,
								area,
								pos,
								anchor_x,
								y,
								PortNameDir::Left,
								name,
							);
						}
					} else {
						// output port on the top edge, offset along width
						if let Some(cell) = buf.cell_mut(Position::new(
							pos.left() + from_port + 1,
							pos.top(),
						)) {
							cell.set_symbol(conn_symbol(
								false,
								ea_node.border_type(),
								ea_conn.line_type(),
								true,
							));
						}
						// Step 9: out-port display name — one cell below the top
						// edge (into the node interior), first char on the inner
						// cell, extending leftward.
						if let Some(name) = ea_node.port_name(ea_conn.from_port) {
							let x = pos.left() + from_port + 1;
							let y = pos.top() + 1;
							write_port_name(
								buf,
								area,
								pos,
								x,
								y,
								PortNameDir::Left,
								name,
							);
						}
					}
				}
			} else {
				// Unplaced node (unreachable / cyclic, or Manual mode with no
				// `set_position`): best-effort id marker in column 0 so the user
				// can see the node exists. Bounds-checked — `set_string` indexes
				// the buffer directly (unlike `cell_mut`), so a small canvas must
				// not panic on a high node id, and the id string is capped to the
				// canvas width so it can't run past the right edge.
				let y = idx_node.0 as u16;
				if y < area.height && area.width > 0 {
					let capped: String = format!("{}", idx_node.0)
						.chars()
						.take(area.width as usize)
						.collect();
					if !capped.is_empty() {
						buf.set_string(0, y, capped, Style::default());
					}
				}
			}
		}
	}
}

impl<'a> ratatui::widgets::StatefulWidget for NodeGraph<'a> {
	type State = crate::FlowState;

	fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
		// Borders/ports/connections: always blit the visible window of the
		// off-screen canvas (rendered once during `calculate` in canvas-absolute
		// coordinates). Using the canvas at every offset — including (0,0) —
		// keeps panning continuous (no jump between "not panned" and "panned")
		// and matches the canvas-absolute content rects from `split_stateful`,
		// so a node's border and its content rect always line up. The canvas
		// holds borders/ports/connections only (no content), so caller-rendered
		// content shows through the blank cells. Call `calculate` first; before
		// it runs the canvas is blank and nothing is drawn.
		blit_canvas(&self.canvas, area, buf, state.view_offset);

		// Selection / hover highlight overlay: recolor the border cells of the
		// targeted node(s). `highlight_border` preserves each cell's symbol (so
		// the port glyphs ├/┬/etc. survive — only fg/modifier changes). Hover is
		// drawn first, then selection, so a node that is both shows the
		// selection. `node_screen_rect` mirrors the placement into the same
		// canvas-absolute space the blit drew, so the highlight lands on the
		// actual border at any pan offset.
		let hover_style = self.highlight_style.add_modifier(Modifier::DIM);
		if let Some(hover_id) = state.hover
			&& let Some(rect) = self.node_screen_rect(hover_id, area, state.view_offset)
		{
			self.highlight_border(buf, rect, hover_style);
		}
		if let Some(sel_id) = state.selection
			&& let Some(rect) = self.node_screen_rect(sel_id, area, state.view_offset)
		{
			self.highlight_border(buf, rect, self.highlight_style);
		}
	}
}
