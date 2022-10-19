use tui::layout::Rect;
use tui::buffer::Buffer;
use tui::style::Style;

pub struct NodeGraph<'a, T: NodeGraphTrait>(pub &'a T);

pub trait NodeGraphTrait {
	/// Returns the number of nodes in the graph
	fn node_count(&self) -> usize;

	/// Returns an iterator over the connections from a requested node.
	///
	/// Would love for this to return an iterator, but I couldn't figure out
	/// how.
	fn connections_from_node(&self, node: usize) -> Vec<Connection>;

	/// Returns an iterator over the connections to a requested node.
	///
	/// See `connections_from_node`
	fn connections_to_node(&self, node: usize) -> Vec<Connection>;

	/// Returns the name of a node
	fn node_name(&self, _node: usize) -> Option<&str> {
		None
	}

	/// Returns the name of a node's port
	fn port_name(&self, _node: usize, _port: usize) -> Option<&str> {
		None
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

impl<'a, T> tui::widgets::StatefulWidget for NodeGraph<'a, T>
where
	T: NodeGraphTrait,
{
//	type State = NodeGraphState;
	type State = ();

	fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
		let mut row = area.y;
		for idx_node in 0..self.0.node_count() {
			let node_name = self.0.node_name(idx_node).unwrap_or("anonymous node");
			buf.set_string(area.x, row, format!("{node_name:?}"), Style::default());
			row += 1;
			for ea_connection in self.0.connections_from_node(idx_node) {
				buf.set_string(area.x + 4, row, format!("{ea_connection:?}"), Style::default());
				row += 1;
			}
			for ea_connection in self.0.connections_to_node(idx_node) {
				buf.set_string(area.x + 4, row, format!("{ea_connection:?}"), Style::default());
				row += 1;
			}
		}
	}
}
