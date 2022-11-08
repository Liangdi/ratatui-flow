#[macro_use] extern crate log;
use std::collections::HashMap as Map;
use std::collections::BTreeSet as Set;
use tui::{
	layout::Rect,
	buffer::Buffer,
	style::Style,
	widgets::{Block, Widget, BorderType, Borders},
};

#[derive(Default)]
pub struct NodeLayout<'a> {
	// minimum size of contents (TODO: doc: including borders?)
	size: (u16, u16),
	block: Block<'a>,
//	in_ports: Vec<PortLayout>,
//	out_ports: Vec<PortLayout>,
}

impl<'a> NodeLayout<'a> {
	pub fn new(size: (u16, u16)) -> Self {
		Self {
			size,
			block: Block::default().border_type(BorderType::Thick).borders(Borders::ALL),
			..Self::default()
		}
	}

	pub fn with_title(mut self, title: &'a str) -> Self {
		self.block = self.block.title(title);
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
		for ea_child in self.get_children(idx_node) {
			if self.placements.contains_key(&ea_child) {
				// nudge it (if necessary)
				self.nudge(ea_child, rect_me.x + rect_me.width);
			}
			else {
				// place it
				self.place_node(ea_child, x + rect_me.width, y, main_chain);
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
			for ea_child in self.get_children(idx_node) {
				assert!(self.placements.contains_key(&ea_child));
				self.nudge(ea_child, x + rect_me.width);
			}
		}
	}

	fn get_children(&self, idx_node: usize) -> Vec<usize> {
		// find children and order them
		let mut children: Vec<_> = self.connections.iter()
			.filter(|ea| { ea.to_node == idx_node })
			.copied()
			.collect()
		;
		children.sort_by(|a,b| a.to_port.cmp(&b.to_port));
		children.iter().map(|ea| ea.from_node).collect()
	}

	pub fn split(&self, area: Rect) -> Vec<Rect> {
		(0..self.nodes.len()).map(|idx_node| {
			self.placements.get(&idx_node).map(|pos| {
				let mut pos = *pos;
				pos.x = area.width.saturating_sub(pos.x + pos.width) + 1;
				pos.y += 1;
				pos.width -= 2;
				pos.height -= 2;
				pos
			})
			.unwrap_or_default()
		}).collect()
	}
}

/*
pub struct NodeGraphState {
	x_offset: usize,
	y_offset: usize,
}
*/

#[derive(Debug, Clone, Copy)]
pub struct Connection {
	pub from_node: usize,
	pub from_port: usize,
	pub to_node: usize,
	pub to_port: usize,
}

impl Connection {
	pub fn new(from_node: usize, from_port: usize, to_node: usize, to_port: usize) -> Self {
		Self { from_node, from_port, to_node, to_port }
	}
}

impl<'a> tui::widgets::StatefulWidget for NodeGraph<'a> {
	// eventually, this will contain stuff like view position
//	type State = NodeGraphState;
	type State = ();

	fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
		for (idx_node, ea_node) in self.nodes.into_iter().enumerate() {
			if let Some(pos) = self.placements.get(&idx_node) {
				let mut pos = *pos;
				pos.x = area.width.saturating_sub(pos.x + pos.width);
				ea_node.block.render(pos, buf);
			}
			else {
				buf.set_string(0, idx_node as u16, format!("{idx_node}"), Style::default());
			}
			/*
			for ea_connection in self.connections.iter().filter(|ea| ea.to_node == idx_node).copied() {
				buf.set_string(block_area.x + 1, row + 1, format!("{ea_connection:?}"), Style::default());
				row += 1;
			}
			buf.set_string(block_area.x + 1, row + 1, format!("{block_area:?}"), Style::default());
			*/
		}
	}
}
