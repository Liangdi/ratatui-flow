use tui::widgets::BorderType;

/// Render information for a single node
#[derive(Debug)]
pub struct NodeLayout<'a> {
	// minimum size of contents (TODO: doc: including borders?)
	pub size: (u16, u16),
	pub border: BorderType,
	title: &'a str,
//	in_ports: Vec<PortLayout>,
//	out_ports: Vec<PortLayout>,
}

impl<'a> NodeLayout<'a> {
	pub fn new(size: (u16, u16)) -> Self {
		Self {
			size,
			border: BorderType::Double,
			title: "",
		}
	}

	pub fn with_title(mut self, title: &'a str) -> Self {
		self.title = title;
		self
	}

	pub fn title(&self) -> &str {
		self.title
	}
}
