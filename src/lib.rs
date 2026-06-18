//! A ratatui widget for rendering **node graphs / flow diagrams** in the terminal.
//!
//! `ratatui-flow` lays out a directed acyclic graph (DAG) of labeled "box + arrow"
//! nodes onto a 2D canvas, auto-routes connections between them, and renders the
//! result through ratatui. It is a robustness-focused fork of
//! [`tui-nodes`](https://git.sr.ht/~jaxter184/tui-nodes).
//!
//! # Headline features
//!
//! - **Automatic DAG layout**: places nodes, avoids overlap, and routes every
//!   connection. The whole graph (borders, ports, connections) is rasterized
//!   once into an off-screen canvas during [`NodeGraph::calculate`].
//! - **A\*-style connection routing**: each [`Connection`] can carry its own
//!   [`LineType`], color, and style.
//! - **Content-adaptive node sizing**: [`NodeLayout::from_content`] sizes a node
//!   to its text via `unicode-width` (correct for CJK / emoji / wide chars).
//! - **Viewport / scrolling**: the native [`NodeGraphView`] + [`Viewport`] +
//!   [`NodeGraph::split_viewport`] trio pans the rasterized canvas without ever
//!   re-running layout.
//! - **Structured diagnostics**: [`Diagnostic`] surfaces non-fatal problems
//!   (unreachable nodes, un-routable connections, bad refs). Nothing here
//!   panics — malformed graphs degrade gracefully.
//! - **Text export**: [`NodeGraph::to_ascii`] (skeleton) and
//!   [`NodeGraph::to_ascii_with`] (skeleton + node bodies) render the graph as
//!   plain ASCII — no terminal, no `Frame`. Great for embedding flowcharts in
//!   docs, Markdown, or CLI output.
//! - **Manual / pinned layout modes**: [`LayoutMode::Manual`] places every node
//!   at an explicit coordinate; [`LayoutMode::Pinned`] treats a subset of
//!   nodes as immovable anchors around which the rest auto-layout. See
//!   [`NodeGraph::set_position`] / [`NodeGraph::with_position`].
//!
//! # Quick start
//!
//! ```no_run
//! use ratatui_flow::{NodeGraph, NodeLayout, Connection};
//!
//! let nodes = vec![
//!     NodeLayout::from_content("Source\n/data/input.csv").with_title("src"),
//!     NodeLayout::from_content("Sink\nINSERT INTO events").with_title("out"),
//! ];
//! let conns = vec![Connection::new(0usize.into(), 0u32.into(), 1u32.into(), 0u32.into())];
//! let mut graph = NodeGraph::new(nodes, conns, 120, 24);
//! graph.calculate();
//!
//! // Render the graph: `split` gives each node's inner content rect, then the
//! // stateful widget draws the borders/ports/connections.
//! //   for (i, z) in graph.split(area).into_iter().enumerate() {
//! //       f.render_widget(Paragraph::new(contents[i]), z);
//! //   }
//! //   f.render_stateful_widget(graph, area, &mut ());
//! # let _ = graph;
//! ```
//!
//! See the [`examples/`](https://github.com/Liangdi/ratatui-flow/tree/main/examples)
//! directory (`basic`, `tiny`, `content`, `viewport`, `export`, `flow_editor`)
//! and the crate's README for more.
//!
//! [`NodeGraph::calculate`]: NodeGraph::calculate
//! [`NodeGraph::to_ascii`]: NodeGraph::to_ascii
//! [`NodeGraph::to_ascii_with`]: NodeGraph::to_ascii_with
//! [`NodeGraph::set_position`]: NodeGraph::set_position
//! [`NodeGraph::with_position`]: NodeGraph::with_position
//! [`NodeGraph::split_viewport`]: NodeGraph::split_viewport

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
mod direction;
pub use direction::FlowDirection;
mod id;
pub use id::{NodeId, PortId};
mod node;
pub use node::NodeLayout;
mod state;
pub use state::FlowState;
mod graph;
#[allow(deprecated)]
pub use graph::{AddNodeError, LayoutMode, NodeGraph, NodeGraphView, Viewport};
