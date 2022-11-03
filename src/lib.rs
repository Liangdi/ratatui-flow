use tui::layout::Rect;
use tui::buffer::Buffer;
use tui::style::Style;
use tui::widgets::{Block, Widget};

#[derive(Default)]
pub struct NodeLayout<'a> {
	pub size: (u16, u16),
	pub block: Option<Block<'a>>,
}

pub struct PortLayout {
}

pub struct NodeGraph<'a, T: NodeGraphTrait>(pub &'a T);

pub trait NodeGraphTrait {
	/// Returns the number of nodes in the graph
	fn node_count(&self) -> usize;

	/// Returns an iterator over the connections to a requested node. Used for
	/// layout.
	///
	/// Would love for this to return an iterator, but I couldn't figure out
	/// how.
	fn connections_to_node(&self, node: usize) -> Vec<Connection>;

	/// Returns a node's data
	fn node(&self, _node: usize) -> NodeLayout;

	/// Returns a port's data
	fn port(&self, _node: usize, _port: usize, is_input: bool) -> PortLayout;
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

impl<'a, T> tui::widgets::StatefulWidget for NodeGraph<'a, T>
where
	T: NodeGraphTrait,
{
	// eventually, this will contain stuff like view position
//	type State = NodeGraphState;
	type State = ();

	fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
		let mut block_position = area.y;
		for idx_node in 0..self.0.node_count() {
			let mut row = block_position;
			let node = self.0.node(idx_node);
			let (width, height) = node.size;
			let block_area = Rect {
				x: area.x,
				y: row,
				width, height,
			};
			if let Some(block) = node.block {
				block.render(block_area, buf);
			}
			for ea_connection in self.0.connections_to_node(idx_node) {
				buf.set_string(block_area.x + 1, row + 1, format!("{ea_connection:?}"), Style::default());
				row += 1;
			}
			buf.set_string(block_area.x + 1, row + 1, format!("{block_area:?}"), Style::default());
			block_position += height
		}
	}
}
