//! Flow direction for graph layout: which way the tree "grows" from root to
//! children, and (independently) whether the main axis is mirrored so the root
//! ends up at the far edge.
//!
//! ## Coordinate model
//!
//! All layout and routing happens in a single off-screen **canvas** coordinate
//! space with origin at the top-left, `x` growing right, `y` growing down.
//! `place_node` always advances the **main axis** (the flow axis) away from the
//! root and stacks siblings along the **cross axis**; a final mirror step flips
//! the main axis onto the screen so the root lands at the configured edge.
//!
//! | Direction | main axis | cross axis | root lands at | mirror main axis? |
//! |-----------|-----------|------------|--------------|-------------------|
//! | `Rtl` (default) | x (horizontal) | y | right edge   | yes               |
//! | `Ltr`           | x (horizontal) | y | left edge    | no                |
//! | `Ttb`           | y (vertical)   | x | top edge     | no                |
//! | `Btt`           | y (vertical)   | x | bottom edge  | yes               |
//!
//! `Rtl` (the default) is byte-for-byte identical to the pre-parameterization
//! hard-coded layout — `FlowDirection::default() == FlowDirection::Rtl`.

use crate::connection::Direction;

/// Which way a [`crate::NodeGraph`] flows: the main axis along which children
/// are laid out away from the root, and which screen edge the root anchors to.
///
/// See the [module docs][self] for the full coordinate model. The default
/// (`Rtl`) preserves the original hard-coded behavior exactly — roots on the
/// right, children flowing leftward.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowDirection {
	/// Left-to-right: root at the left edge, children flow rightward.
	Ltr,
	/// Right-to-left: root at the right edge, children flow leftward.
	/// (The original hard-coded behavior; the default.)
	#[default]
	Rtl,
	/// Top-to-bottom: root at the top edge, children flow downward.
	Ttb,
	/// Bottom-to-top: root at the bottom edge, children flow upward.
	Btt,
}

impl FlowDirection {
	/// `true` when the main axis is horizontal (`Ltr`/`Rtl`); `false` when it is
	/// vertical (`Ttb`/`Btt`). Determines whether the main axis is `x` and the
	/// cross axis is `y`, or vice versa.
	pub(crate) fn is_horizontal(self) -> bool {
		matches!(self, FlowDirection::Ltr | FlowDirection::Rtl)
	}

	/// The canvas-axis [`Direction`] a connection **leaves** the `from` node on
	/// (the out-port side). This is the entry direction assigned to the pathing
	/// `start` cell. It must point at an edge of the port cell that is **not**
	/// blocked by the node's `block_zone`, i.e. toward the open side the
	/// connection runs into.
	///
	/// Matches the pre-parameterization hard-coded values for `Ltr`/`Rtl`
	/// (`West`); the vertical directions pick the open-facing side after the
	/// main-axis mirror (`North` for `Ttb`, `South` for `Btt`).
	pub(crate) fn main_out_direction(self) -> Direction {
		match self {
			FlowDirection::Ltr | FlowDirection::Rtl => Direction::West,
			FlowDirection::Ttb => Direction::North,
			FlowDirection::Btt => Direction::South,
		}
	}

	/// The canvas-axis [`Direction`] a connection **enters** the `to` node on
	/// (the in-port side): the opposite of [`main_out_direction`][Self::main_out_direction].
	pub(crate) fn main_in_direction(self) -> Direction {
		match self.main_out_direction() {
			Direction::East => Direction::West,
			Direction::West => Direction::East,
			Direction::North => Direction::South,
			Direction::South => Direction::North,
		}
	}

	/// Whether the main axis is mirrored when transforming canvas→screen
	/// coordinates (i.e. the root ends up at the *far* edge rather than the
	/// origin edge). `Rtl` and `Btt` mirror; `Ltr` and `Ttb` do not.
	pub(crate) fn mirror_main_axis(self) -> bool {
		matches!(self, FlowDirection::Rtl | FlowDirection::Btt)
	}
}
