# ratatui-flow

[中文](README.md) | English

[![crates.io](https://img.shields.io/crates/v/ratatui-flow.svg)](https://crates.io/crates/ratatui-flow)
[![docs.rs](https://docs.rs/ratatui-flow/badge.svg)](https://docs.rs/ratatui-flow)

Node graph / flow diagram widgets for [ratatui](https://crates.io/crates/ratatui) — lay out
boxes-and-arrows diagrams (DAGs) in the terminal, with auto-routed connections, content-aware
node sizing, and structured diagnostics.

```
                      ╭src────────────╮        ╭parse───────╮     ╭xform─────────────╮
                      │Source         ├──┬─────┥Parse       ╞═════╡Transform         ├──╮
                      │/data/input.csv│  │     │header row  ├─╮╭──┤normalize -> [0,1]│  │
                      ╰───────────────╯  │     ╰────────────╯ ││  ╰──────────────────╯  │
                                         │  ╭valid──────────╮ ││                        │
                                         ╰──┤Validate       ├─│╯                        ╰── ...
                                            ╰───────────────╯
```

## Features

- **Automatic layout** of node graphs (DAGs): place nodes, avoid overlaps, route connections.
- **Auto-routed connections** via grid-based search, with per-connection line type / color / style.
- **Content-aware node sizing** via `NodeLayout::from_content`, measuring display width
  (CJK / emoji aware through `unicode-width`).
- **Graceful degradation**: cycles, out-of-bounds node refs, or too-small canvases never panic —
  they degrade gracefully and surface a `Diagnostic`.
- **Minimal, sealed public API**: `NodeGraph`, `NodeLayout`, `Connection`, `LineType`
  (+ `Diagnostic`). Internal layout types are not exposed.

## Quick start

`Cargo.toml`:

```toml
[dependencies]
ratatui = { version = "0.30", default-features = false }
ratatui-flow = "0.1"
```

```rust
use ratatui::widgets::Paragraph;
use ratatui_flow::{NodeGraph, NodeLayout, Connection};

let nodes = vec![
    NodeLayout::from_content("Source\n/data/input.csv").with_title("src"),
    NodeLayout::from_content("Sink\nINSERT INTO events").with_title("out"),
];
let conns = vec![Connection::new(0, 0, 1, 0)];

let mut graph = NodeGraph::new(nodes, conns, 120, 24);
graph.calculate();

// surface any non-fatal issues (unreachable nodes, failed routing, ...)
for d in graph.diagnostics() {
    eprintln!("{d:?}");
}
```

Render it with ratatui:

```rust
// zones[i] is the inner content rect of node i
let zones = graph.split(area);
for (i, z) in zones.into_iter().enumerate() {
    f.render_widget(Paragraph::new(contents[i]), z);
}
f.render_stateful_widget(graph, area, &mut ());
```

## Examples

```bash
cargo run --example viewport   # interactive: 16-node graph + keyboard scrolling
cargo run --example content    # 6-node pipeline, content-aware sizing
cargo run --example basic      # minimal
cargo run --example tiny       # renders into a buffer and prints
```

`viewport` demonstrates an application-level viewport (render once to a large off-screen
buffer, then blit a scrolled window each frame), since the library itself does not yet have a
built-in viewport. Controls: `hjkl` / arrows to scroll, `PgUp`/`PgDn`, `Home`, `q`/`Esc` to quit.

## Diagnostics

Call `graph.diagnostics()` after `calculate()` to get `&[Diagnostic]`:

| Variant | Meaning |
|---|---|
| `UnplacedNode { node }` | node is unreachable from any root (e.g. sits in a pure cycle) |
| `InvalidConnectionRef { from_node, to_node }` | connection references an out-of-bounds node, skipped |
| `RoutingFailed { from_node, from_port, to_node, to_port }` | a connection could not be routed and fell back to an alias character |

## API surface

| Item | Purpose |
|---|---|
| `NodeGraph` | Owns nodes + connections. `new` / `calculate` / `split` / `diagnostics` + ratatui `StatefulWidget`. |
| `NodeLayout` | One node's render info. `new((w,h))` or `from_content(text)` + builders. |
| `Connection` | `new(from_node, from_port, to_node, to_port)` + `with_line_type` / `with_line_style`. |
| `LineType` | `Plain` / `Rounded` / `Double` / `Thick`. |
| `Diagnostic` | Observable layout/routing issues. |

## Acknowledgements

`ratatui-flow` is a fork of [`tui-nodes`](https://git.sr.ht/~jaxter184/tui-nodes) by
[jaxter184](https://git.sr.ht/~jaxter184), renamed and extended with robustness fixes, a test
suite, a tightened public API, structured diagnostics, and content-aware node sizing.

## License

MIT (see [LICENSE](LICENSE)).
