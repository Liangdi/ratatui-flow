# ratatui-flow

中文 | [English](README.en.md)

[![crates.io](https://img.shields.io/crates/v/ratatui-flow.svg)](https://crates.io/crates/ratatui-flow)
[![docs.rs](https://docs.rs/ratatui-flow/badge.svg)](https://docs.rs/ratatui-flow)

[ratatui](https://crates.io/crates/ratatui) 的节点图 / 流程图 widget —— 在终端里布局“框 + 箭头”的有向无环图（DAG），支持连接线自动布线、节点按内容自适应尺寸，以及结构化的诊断信息。

```
                      ╭src────────────╮        ╭parse───────╮     ╭xform─────────────╮
                      │Source         ├──┬─────┥Parse       ╞═════╡Transform         ├──╮
                      │/data/input.csv│  │     │header row  ├─╮╭──┤normalize -> [0,1]│  │
                      ╰───────────────╯  │     ╰────────────╯ ││  ╰──────────────────╯  │
                                         │  ╭valid──────────╮ ││                        │
                                         ╰──┤Validate       ├─│╯                        ╰── ...
                                            ╰───────────────╯
```

## 特性

- **自动布局** 节点图（DAG）：放置节点、避免重叠、布线连接。
- **连接自动布线**：基于网格搜索，每条连接可单独设置线型 / 颜色 / 样式。
- **节点按内容自适应尺寸**：`NodeLayout::from_content` 按文本显示宽度计算（通过 `unicode-width`，正确处理 CJK / emoji 等宽字符）。
- **优雅降级**：环、越界的节点引用、过小的画布都不会 panic，而是优雅处理并通过 `Diagnostic` 上报。
- **最小化、封闭的公共 API**：`NodeGraph`、`NodeLayout`、`Connection`、`LineType`（外加 `Diagnostic`）；内部布局类型不对外暴露。

## 快速开始

`Cargo.toml`：

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

// 上报非致命问题（不可达节点、布线失败……）
for d in graph.diagnostics() {
    eprintln!("{d:?}");
}
```

用 ratatui 渲染：

```rust
// zones[i] 是第 i 个节点的内部内容区域
let zones = graph.split(area);
for (i, z) in zones.into_iter().enumerate() {
    f.render_widget(Paragraph::new(contents[i]), z);
}
f.render_stateful_widget(graph, area, &mut ());
```

## 示例

```bash
cargo run --example viewport   # 交互式：16 节点图 + 键盘滚动视口
cargo run --example content    # 6 节点流水线，内容自适应尺寸
cargo run --example basic      # 最小示例
cargo run --example tiny       # 渲染到缓冲区并打印
```

`viewport` 演示了应用层的视口实现：把整张图一次性渲染到一个大的离屏 buffer，每帧只 blit 滚动后可见的那一块（库本身暂未内置视口）。操作：`hjkl` / 方向键滚动、`PgUp`/`PgDn`、`Home`、`q`/`Esc` 退出。

## 诊断

`calculate()` 之后调用 `graph.diagnostics()` 获取 `&[Diagnostic]`：

| 变体 | 含义 |
|---|---|
| `UnplacedNode { node }` | 节点从任何根都不可达（例如位于纯环中），未被放置 |
| `InvalidConnectionRef { from_node, to_node }` | 连接引用了越界的节点索引，已跳过 |
| `RoutingFailed { from_node, from_port, to_node, to_port }` | 连接无法布线，降级为别名字符显示 |

## API 概览

| 条目 | 用途 |
|---|---|
| `NodeGraph` | 持有节点 + 连接；`new` / `calculate` / `split` / `diagnostics` + 实现 ratatui `StatefulWidget`。 |
| `NodeLayout` | 单个节点的渲染信息；`new((w,h))` 或 `from_content(text)` + builder。 |
| `Connection` | `new(from_node, from_port, to_node, to_port)` + `with_line_type` / `with_line_style`。 |
| `LineType` | `Plain` / `Rounded` / `Double` / `Thick`。 |
| `Diagnostic` | 可观测的布局 / 布线问题。 |

## 致谢

`ratatui-flow` 是 [`tui-nodes`](https://git.sr.ht/~jaxter184/tui-nodes)（作者 [jaxter184](https://git.sr.ht/~jaxter184)）的 fork，在此基础上去除 panic、补齐测试、收紧公共 API，并加入结构化诊断与内容自适应尺寸。

## 许可证

MIT（见 [LICENSE](LICENSE)）。
