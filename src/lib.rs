use std::collections::HashMap as Map;
use std::collections::BTreeSet as Set;
use tui::layout::Margin;
use tui::style::Color;
use tui::style::Modifier;
use tui::{
	layout::Rect,
	buffer::Buffer,
	style::Style,
	widgets::Widget,
};

mod connection;
use connection::*;
pub use connection::Connection;
mod node;
pub use node::NodeLayout;
mod graph;
pub use graph::*;
