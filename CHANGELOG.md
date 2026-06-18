## [unreleased]

### 🚀 Features

- *(graph)* Text export: `NodeGraph::to_ascii()` (skeleton) and `to_ascii_with(content)` (skeleton + node bodies) render the graph as plain ASCII text — no terminal required
- *(graph)* `LayoutMode::Manual` mode: place every node at an explicit coordinate via `set_position` / `with_position`
- *(graph)* `LayoutMode::Pinned` mode: pin a subset of nodes as immovable anchors around which the rest auto-layout

### 🐛 Bug Fixes

- *(graph)* `calculate()` no longer panics on graphs that don't fit the canvas: unplaced nodes (cycles / unreachable, or `Manual` mode without a position) and nodes larger than the canvas now degrade gracefully. The unplaced-node fallback wrote each node's id at row `node_id` with no bounds check, indexing the canvas buffer out of bounds on small canvases; it is now bounds-checked. The A* router's port guard also tightened (`>` → `>=`) so a port landing exactly on the canvas edge can't index the edge grid out of bounds

### 📚 Documentation

- Bilingual README (Chinese primary + English version)
## [0.1.0] - 2022-11-13
