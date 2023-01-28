use tui::{
	style::Style,
	widgets::BorderType,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
	North,
	South,
	East,
	#[allow(unused)]
	West,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Connection {
	pub from_node: usize,
	pub from_port: usize,
	pub to_node: usize,
	pub to_port: usize,
}

impl Connection {
	pub fn new(from_node: usize, from_port: usize, to_node: usize, to_port: usize) -> Self {
		Self { from_node, from_port, to_node, to_port, }
	}
}

#[derive(Debug, Clone)]
pub struct ConnectionLayout {
	start_pos: (u16, u16),
	points: Vec<(Direction, u16)>,
	border: BorderType,
	style: Style,
}

impl ConnectionLayout {
	pub fn new(start_pos: (u16, u16)) -> Self {
		Self {
			start_pos,
			points: Vec::new(),
			border: BorderType::Rounded,
			style: Style::default(),
		}
	}

	pub fn start_pos(&self) -> (u16, u16) {
		self.start_pos
	}
	pub fn style(&self) -> Style {
		self.style
	}
	pub fn border(&self) -> BorderType {
		self.border
	}
	pub fn push(&mut self, item: (Direction, u16)) {
		self.points.push(item)
	}
	pub fn points(&self) -> &Vec<(Direction, u16)> {
		&self.points
	}
}

/// Generate the correct connection symbol for this node
pub fn conn_symbol(is_input: bool, block_style: BorderType, conn_style: BorderType) -> &'static str {
	let out = match (block_style, conn_style) {
		(BorderType::Plain
		|BorderType::Rounded, BorderType::Thick)   => ("┥", "┝"),
		(BorderType::Plain
		|BorderType::Rounded, BorderType::Double)  => ("╡", "╞"),
		(BorderType::Plain
		|BorderType::Rounded, BorderType::Plain
		                    | BorderType::Rounded) => ("┤", "├"),

		(BorderType::Thick,   BorderType::Thick)   => ("┫", "┣"),
		(BorderType::Thick,   BorderType::Double)  => ("X", "X"),
		(BorderType::Thick,   BorderType::Plain
		                    | BorderType::Rounded) => ("┨", "┠"),

		(BorderType::Double,  BorderType::Thick)   => ("X", "X"),
		(BorderType::Double,  BorderType::Double)  => ("╣", "╠"),
		(BorderType::Double,  BorderType::Plain
		                    | BorderType::Rounded) => ("╢", "╟"),
	};
	if is_input { out.0 } else { out.1 }
}

pub const ALIAS_CHARS: [&str; 24] = [
	"α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ", "ν", "ξ", "ο", "π", "ρ", "σ", "τ", "υ", "φ", "χ", "ψ", "ω",
];
