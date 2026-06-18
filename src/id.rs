//! Stable, opaque identity types for nodes and ports (backed by `u32`).

/// Stable, opaque identity of a node in a [`NodeGraph`][crate::NodeGraph].
///
/// Internally a `u32`; the field is `pub(crate)` so graph/connection internals
/// can read it directly without getter noise, while external callers see an
/// opaque token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
	/// The underlying `u32` value of this id. Exposed so callers can map a
	/// [`NodeId`] back to their own data (e.g. as an index into a content
	/// array) without keeping a parallel `Map<NodeId, _>`.
	pub fn as_u32(self) -> u32 {
		self.0
	}
}

/// Stable, opaque identity of a port on a node (input or output).
///
/// Internally a `u32`; see [`NodeId`] for the visibility rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortId(pub(crate) u32);

impl PortId {
	/// The underlying `u32` value of this id.
	pub fn as_u32(self) -> u32 {
		self.0
	}
}

impl From<usize> for NodeId {
	fn from(value: usize) -> Self {
		Self(value as u32)
	}
}

impl From<u32> for NodeId {
	fn from(value: u32) -> Self {
		Self(value)
	}
}

impl From<usize> for PortId {
	fn from(value: usize) -> Self {
		Self(value as u32)
	}
}

impl From<u32> for PortId {
	fn from(value: u32) -> Self {
		Self(value)
	}
}

impl std::fmt::Display for NodeId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Node#{}", self.0)
	}
}

impl std::fmt::Display for PortId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Port#{}", self.0)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn node_id_from_usize() {
		let id: NodeId = 3usize.into();
		assert_eq!(id, NodeId(3));
	}

	#[test]
	fn node_id_from_u32() {
		let id: NodeId = 42u32.into();
		assert_eq!(id, NodeId(42));
	}

	#[test]
	fn port_id_from_usize() {
		let id: PortId = 1usize.into();
		assert_eq!(id, PortId(1));
	}

	#[test]
	fn port_id_from_u32() {
		let id: PortId = 7u32.into();
		assert_eq!(id, PortId(7));
	}

	#[test]
	fn equality() {
		assert_eq!(NodeId(5), NodeId(5));
		assert_ne!(NodeId(5), NodeId(6));
		assert_eq!(PortId(2), PortId(2));
		assert_ne!(PortId(2), PortId(3));
	}

	#[test]
	fn ordering() {
		let mut ids = [NodeId(3), NodeId(1), NodeId(2)];
		ids.sort();
		assert_eq!(ids, [NodeId(1), NodeId(2), NodeId(3)]);

		let mut ports = [PortId(9), PortId(0), PortId(4)];
		ports.sort();
		assert_eq!(ports, [PortId(0), PortId(4), PortId(9)]);
	}

	#[test]
	fn node_id_display() {
		assert_eq!(format!("{}", NodeId(3)), "Node#3");
		assert_eq!(format!("{}", NodeId(0)), "Node#0");
	}

	#[test]
	fn port_id_display() {
		assert_eq!(format!("{}", PortId(1)), "Port#1");
		assert_eq!(format!("{}", PortId(12)), "Port#12");
	}
}
