# Screenshots

Rendered gallery of the example TUIs. Captured by driving each example through
a PTY, replaying the byte stream through `pyte`, and rasterizing the final
screen with PIL (truecolor preserved).

## Flow-graph examples (ratatui-sci-fi themed, `Rtl` so source → sink reads left → right)

| File | Example | Theme |
|---|---|---|
| `ci_pipeline.png` | `ci_pipeline` — CI/CD release flow: commit → build → {test, lint} → package → staging → prod | DeepSpace |
| `ml_pipeline.png` | `ml_pipeline` — ML/data pipeline: ingest → validate → {clean, enrich} → features → train → eval → deploy | Nebula |
| `services.png` | `services` — microservice request flow: client → gateway → {auth, limit} → {orders, payments} → {db, cache} | Cyberpunk |

## Interactive editor (`flow_editor`)

`flow_editor.png` cycles the eight built-in themes with the `t` key:

| File | Theme |
|---|---|
| `flow_editor_deepspace.png` | DeepSpace (default) |
| `flow_editor_bloodmoon.png` | Bloodmoon |
| `flow_editor_nebula.png` | Nebula |
| `flow_editor_arctic.png` | Arctic |

Re-capture any of these with `python3 /tmp/shot.py` / `python3 /tmp/shots.py`
(the PTY capture harness).
