# Formatter config (`.wsformat.toml`)

Project-level formatter style, read by both the `wsformat` CLI and the LSP. TOML, flat keys.

Discovery walks up from the file's directory; `.wsformat.toml` wins over `wsformat.toml`, nearest ancestor first. Unset keys fall back to the editor's settings (LSP) or built-in defaults (CLI). CLI flags override the file. Unknown keys are an error.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `tab_size` | int | `4` | Spaces per indent (ignored when `use_tabs`). |
| `use_tabs` | bool | `false` | Indent with tabs. |
| `line_limit` | int | `100` | Soft wrap width. |
| `colon_spacing` | `"spaced"` \| `"compact"` | `"spaced"` | `x : int` vs `x: int`. |
| `align_member_colons` | bool | `false` | Align `:` across consecutive member declarations. |
| `annotation_placement` | `"preserve"` \| `"ownLine"` \| `"sameLine"` | `"preserve"` | `@addField` placement. |
| `default_placement` | `"preserve"` \| `"ownLine"` \| `"sameLine"` | `"preserve"` | Trailing `default` placement. |

```toml
use_tabs = true
colon_spacing = "compact"
```
