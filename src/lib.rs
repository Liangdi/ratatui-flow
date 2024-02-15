use std::collections::HashMap as Map;
use std::collections::BTreeSet as Set;
use ratatui::layout::Margin;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::{
	layout::Rect,
	buffer::Buffer,
	style::Style,
	widgets::Widget,
};

mod connection;
use connection::*;
pub use connection::{Connection, LineType};
mod node;
pub use node::NodeLayout;
mod graph;
pub use graph::*;
