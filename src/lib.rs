use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use std::collections::BTreeSet as Set;
use std::collections::HashMap as Map;

mod connection;
use connection::*;
pub use connection::{Connection, Diagnostic, LineType};
mod id;
pub use id::{NodeId, PortId};
mod node;
pub use node::NodeLayout;
mod graph;
pub use graph::{NodeGraph, NodeGraphView, Viewport};
