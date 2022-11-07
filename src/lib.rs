use tui::layout::Rect;
use tui::buffer::Buffer;
use tui::style::Style;
use tui::widgets::{Block, Widget, BorderType, Borders};

#[derive(Default)]
pub struct NodeLayout<'a> {
	size: (u16, u16),
	block: Option<Block<'a>>,
//	in_ports: Vec<PortLayout>,
//	out_ports: Vec<PortLayout>,
}

impl<'a> NodeLayout<'a> {
	pub fn new(size: (u16, u16)) -> Self {
		Self {
			size,
			block: Some(Block::default().border_type(BorderType::Double).borders(Borders::ALL)),
			..Self::default()
		}
	}

	pub fn with_title(mut self, title: &'a str) -> Self {
		self.block = Some(self.block.unwrap_or(Block::default()).title(title));
		self
	}
}

/*
pub struct PortLayout {
}
*/

#[derive(Default)]
pub struct NodeGraph<'a>{
	nodes: Vec<NodeLayout<'a>>,
	connections: Vec<Connection>,
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
		let mut block_position = area.y;
		for (idx_node, ea_node) in self.nodes.into_iter().enumerate() {
			let mut row = block_position;
			let (width, height) = ea_node.size;
			let block_area = Rect {
				x: area.x,
				y: row,
				width, height,
			};
			if let Some(block) = ea_node.block {
				block.render(block_area, buf);
			}
			for ea_connection in self.connections.iter().filter(|ea| ea.to_node == idx_node).copied() {
				buf.set_string(block_area.x + 1, row + 1, format!("{ea_connection:?}"), Style::default());
				row += 1;
			}
			buf.set_string(block_area.x + 1, row + 1, format!("{block_area:?}"), Style::default());
			block_position += height
		}
	}
}
