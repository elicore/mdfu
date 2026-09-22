---
title: Configuration
description: "Optional TUI theme file: XDG path, precedence, and every key."
---

mdfu works with zero configuration. An optional YAML file restyles the interactive [TUI](/guide/tui/): colors, pill shape, frontmatter visibility, and how many body lines the preview renders. The file is read once at startup and only affects the TUI; non-interactive filter mode ignores it.

## Location and precedence

The theme resolves from the first source that applies:

1. `--config PATH` on the command line (see [CLI](/reference/cli/)).
2. The `$MDFU_CONFIG` environment variable.
3. The discovered XDG file: `$XDG_CONFIG_HOME/mdfu/config.yaml` (`~/.config/mdfu/config.yaml` when `$XDG_CONFIG_HOME` is unset).
4. Builtin defaults that reproduce the stock look.

When no file exists, the builtin defaults apply, so the file is never required. A file requested explicitly (via `--config` or `$MDFU_CONFIG`) that is missing, unreadable, or malformed is an error. A malformed discovered file is ignored with a single warning line, and the defaults apply.

Unknown keys are ignored. Invalid enum values keep the builtin default, and numeric settings are clamped into their supported ranges.

Run `mdfu --config default` to print the builtin defaults — every key below with its default value — to stdout, then copy and edit the result.

## Keys

Colors are xterm 256-color palette indices, given as strings (quote them in YAML). SGR settings are raw parameter bytes, e.g. `"1;30;103"`.

| Key | Values | Default | Meaning |
|---|---|---|---|
| `markdown_style` | `dark` \| `light` | auto (empty) | Glamour style for the preview body; empty lets glamour pick. |
| `show_frontmatter` | bool | `true` | Frontmatter block visible at startup; `ctrl+f` toggles it live. |
| `body_lines` | int 1..200 | `30` | Body lines shown in the preview. |
| `cursor_color` | color | `212` | Cursor row in the result list (bold). |
| `selected_color` | color | `82` | Multi-selected rows in the result list (bold). |
| `title_color` | color | `212` | Document title in the preview (bold). |
| `filename_color` | color | `244` | Filename header at the top of the preview. |
| `preview_header_color` | color | unset (bold + underline) | Preview header style; setting it adds this foreground. |
| `frontmatter_key_color` | color | `245` | Labels of the frontmatter rows. |
| `dim_faint` | bool | `true` | Faint rendering for dimmed text such as status hints. |
| `pill_foreground` | color | `231` | Tag/list pill text. |
| `pill_background` | color | `62` | Tag/list pill background. |
| `pill_shape` | `none` \| `round` | `round` | Pill border shape. |
| `pill_padding` | int 0..2 | `1` | Horizontal padding inside pills. |
| `highlight_sgr` | SGR params | `"1;30;103"` | Query-match emphasis (bold, black on bright yellow). |
| `link_sgr` | SGR params | `"1;4;38;5;212"` | Hyperlink labels in the preview. |

## Example

```yaml
# ~/.config/mdfu/config.yaml
markdown_style: light
show_frontmatter: false
body_lines: 50
title_color: "99"
pill_shape: none
highlight_sgr: "1;30;113"
```

See also: [TUI](/guide/tui/) for the interactive picker this styles, and [CLI](/reference/cli/) for the `--config` flag.
