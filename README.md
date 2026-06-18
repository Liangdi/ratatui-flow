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
- **最小化、封闭的公共 API**：`NodeGraph`、`NodeGraphView`、`Viewport`、`NodeLayout`、`Connection`、`LineType`（外加 `Diagnostic`）；内部布局类型不对外暴露。
- **原生视口 / 滚动**：图在 `calculate()` 时一次性渲染到离屏 canvas，之后用 `NodeGraphView`（blit 滚动窗口）+ `split_viewport`（拿节点内容的屏幕坐标 rect）即可平移/滚动，无需每帧重算布局。

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
// from/to 用稳定身份 NodeId/PortId(usize 可 .into() 隐式转换)
let conns = vec![Connection::new(0.into(), 0.into(), 1.into(), 0.into())];

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
cargo run --example export     # 非交互：把图导出为 ASCII 文本
cargo run --example basic      # 最小示例
cargo run --example tiny       # 渲染到缓冲区并打印
```

`viewport` 演示了库原生的视口 API：`NodeGraphView`（blit 滚动后的边框/端口/连线）+ `split_viewport`（拿节点内容的屏幕坐标 rect）。整张图在 `calculate()` 时渲染到离屏 canvas 一次，之后滚动只是改 `Viewport` 的 offset。操作：`hjkl` / 方向键滚动、`PgUp`/`PgDn`、`Home`、`q`/`Esc` 退出。

## 视口 / 滚动

当图比屏幕大时，用库原生的视口 API。整张图（边框/端口/连线，不含节点内容）在 `calculate()` 时一次性渲染到内部离屏 canvas；之后每帧只需：

1. `split_viewport(area, &viewport)` 拿到每个节点内容的**屏幕坐标** rect（已按 offset 平移并裁剪到 `area`；不可见为 0×0）；
2. 渲染 `NodeGraphView`（把 canvas 的可见窗口按 offset blit 到屏幕）。

```rust
use ratatui::widgets::{Paragraph, Widget};
use ratatui_flow::{NodeGraph, NodeGraphView, Viewport};

// 图比屏幕大；calculate() 时整图画到离屏 canvas 一次
let mut graph = NodeGraph::new(nodes, conns, 220, 110);
graph.calculate();

let mut viewport = Viewport::new();   // offset = (0, 0)
// ... 事件循环里按方向键改 viewport.offset.0 / .1 ...

// 每帧：
let zones = graph.split_viewport(view_area, &viewport);
for (i, z) in zones.iter().enumerate() {
    if z.width > 0 && z.height > 0 {
        f.render_widget(Paragraph::new(contents[i]), *z);   // 你自己渲染节点内容
    }
}
f.render_widget(NodeGraphView::new(&graph).viewport(viewport), view_area);  // 边框/连线
```

`NodeGraphView` 持有 `&graph`，所以不需要每帧 clone 图；布局/布线从不重算。

## 诊断

`calculate()` 之后调用 `graph.diagnostics()` 获取 `&[Diagnostic]`：

| 变体 | 含义 |
|---|---|
| `UnplacedNode { node }` | 节点从任何根都不可达（例如位于纯环中），未被放置 |
| `InvalidConnectionRef { from_node, to_node }` | 连接引用了越界的节点索引，已跳过 |
| `RoutingFailed { from_node, from_port, to_node, to_port }` | 连接无法布线，降级为别名字符显示 |

## 文本导出

把图渲染成纯 ASCII 文本——无需终端、无需 `Frame`，适合嵌入文档 / Markdown / CLI 输出。`to_ascii()` 只画骨架（边框/端口/连线），`to_ascii_with(content)` 在骨架之上叠加每个节点的内容。两者都是只读操作，需先调用 `calculate()`。

```rust
use ratatui_flow::{NodeGraph, NodeLayout, Connection};

let nodes = vec![
    NodeLayout::from_content("Source\n/data/input.csv").with_title("src"),
    NodeLayout::from_content("Sink\nINSERT INTO events").with_title("out"),
];
let conns = vec![Connection::new(0.into(), 0.into(), 1.into(), 0.into())];
let mut graph = NodeGraph::new(nodes, conns, 80, 8);
graph.calculate();

// 骨架（不含内容）
println!("{}", graph.to_ascii());
// 骨架 + 节点内容（按 id 查表返回每节点正文）
let bodies = ["Source\n/data/input.csv", "Sink\nINSERT INTO events"];
println!("{}", graph.to_ascii_with(|id| Some(bodies[id.as_u32() as usize])));
```

效果（完整版）：

```
┌src────────────┐     ┌out──────────────┐
│Source         ├─────┤Sink             │
│/data/input.csv│     │INSERT INTO events│
└───────────────┘     └─────────────────┘
```

运行 `cargo run --example export` 可看到完整 4 节点流水线的骨架与完整两种输出。

## 布局模式

默认 `LayoutMode::Auto`：每次 `calculate()` 自动布局全部节点。另外两种模式由你显式指定节点位置：

- **`LayoutMode::Manual`**：每个节点都必须通过 `set_position`（或 `with_position`）给出坐标；任何缺失的节点会触发 `UnplacedNode` 诊断，但**不会** panic。
- **`LayoutMode::Pinned`**：只有**部分**节点通过 `set_position` 被钉住，它们是不可移动的锚点 / 障碍，其余节点在它们之间自动布局。

```rust
use ratatui_flow::{NodeGraph, NodeLayout, Connection, LayoutMode};

// Manual：所有节点位置由你决定
let mut g = NodeGraph::new(nodes, conns, 120, 40)
    .with_layout_mode(LayoutMode::Manual)
    .with_position(0.into(), 2, 2)
    .with_position(1.into(), 40, 2);
g.calculate();

// Pinned：只钉住少数节点，其余自动避让
let mut g = NodeGraph::new(nodes2, conns2, 120, 40)
    .with_layout_mode(LayoutMode::Pinned)
    .with_position(0.into(), 2, 2);   // 节点 0 固定，其余自动布局
g.calculate();
```

v1 限制：当钉住的节点之间碰撞配置极端时，自动布局会优雅降级（通过 `Diagnostic` 上报，绝不 panic）。

## API 概览

| 条目 | 用途 |
|---|---|
| `NodeGraph` | 持有节点 + 连接；`new` / `calculate` / `split` / `split_viewport` / `diagnostics` + 实现 ratatui `StatefulWidget`。`calculate()` 时整图（边框/端口/连线）渲染到内部离屏 canvas。 |
| `NodeGraphView` | 一个 ratatui `Widget`：按 `Viewport` 的 offset 把 canvas 的可见窗口 blit 到屏幕（仅边框/端口/连线，不含节点内容）。 |
| `Viewport` | 视口在 canvas 中的左上角 offset `(x, y)`；传给 `split_viewport` 与 `NodeGraphView`。 |
| `NodeLayout` | 单个节点的渲染信息；`new((w,h))` 或 `from_content(text)` + builder。 |
| `Connection` | `new(from_node, from_port, to_node, to_port)` + `with_line_type` / `with_line_style`。 |
| `LineType` | `Plain` / `Rounded` / `Double` / `Thick`。 |
| `Diagnostic` | 可观测的布局 / 布线问题。 |

## 致谢

`ratatui-flow` 是 [`tui-nodes`](https://git.sr.ht/~jaxter184/tui-nodes)（作者 [jaxter184](https://git.sr.ht/~jaxter184)）的 fork，在此基础上去除 panic、补齐测试、收紧公共 API，并加入结构化诊断与内容自适应尺寸。

## 许可证

MIT（见 [LICENSE](LICENSE)）。
