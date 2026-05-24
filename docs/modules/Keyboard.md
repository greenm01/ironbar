> [!NOTE]
> This module requires your user is in the `input` group.

> [!IMPORTANT]
> The keyboard layout feature is only available on Sway, Hyprland, and Triad.

Triad support uses native Triad IPC. `TRIAD_SOCKET` is used when set; otherwise Ironbar looks for `triad.sock` under `XDG_RUNTIME_DIR`.

Displays the toggle state of the capslock, num lock and scroll lock keys, and the current keyboard layout.

![Screenshot of keyboard widget](https://f.jstanger.dev/github/ironbar/keys.png)

## Configuration

> Type: `keyboard`

| Name               | Type                           | Default | Description                                                                                                               |
| ------------------ | ------------------------------ | ------- | ------------------------------------------------------------------------------------------------------------------------- |
| `show_caps`        | `boolean`                      | `true`  | Whether to show capslock indicator.                                                                                       |
| `show_num`         | `boolean`                      | `true`  | Whether to show num lock indicator.                                                                                       |
| `show_scroll`      | `boolean`                      | `true`  | Whether to show scroll lock indicator.                                                                                    |
| `show_layout`      | `boolean`                      | `true`  | Whether to show the keyboard layout button.                                                                               |
| `icon_size`        | `integer`                      | `32`    | Size to render icon at (image icons only).                                                                                |
| `icons.caps_on`    | `string` or [image](images)    | `󰪛`     | Icon to show for enabled capslock indicator.                                                                              |
| `icons.caps_off`   | `string` or [image](images)    | `''`    | Icon to show for disabled capslock indicator.                                                                             |
| `icons.num_on`     | `string` or [image](images)    | ``     | Icon to show for enabled num lock indicator.                                                                              |
| `icons.num_off`    | `string` or [image](images)    | `''`    | Icon to show for disabled num lock indicator.                                                                             |
| `icons.scroll_on`  | `string` or [image](images)    | ``     | Icon to show for enabled scroll lock indicator.                                                                           |
| `icons.scroll_off` | `string` or [image](images)    | `''`    | Icon to show for disabled scroll lock indicator.                                                                          |
| `icons.layout_map` | `Map<string, string or image>` | `{}`    | Map of icons or labels to show for a particular keyboard layout. Layouts use their actual name if not present in the map. Layouts are matched in the order they appear in the map. If a pattern to match ends with a `*`, it acts as a wildcard, matching any layout name that begins with the part before the `*`. |
| `seat`             | `string`                       | `seat0` | ID of the Wayland seat to attach to.                                                                                      |

<details>
<summary>JSON</summary>

```json
{
  "end": [
    {
      "type": "keyboard",
      "show_scroll": false,
      "icons": {
        "caps_on": "󰪛",
        "layout_map": {
          "English (US)": "🇺🇸",
          "Ukrainian": "🇺🇦"
        }
      }
    }
  ]
}
```

</details>

<details>
<summary>TOML</summary>

```toml
[[end]]
type = "keyboard"
show_scroll = false

[end.icons]
caps_on = "󰪛"

[end.icons.layout_map]
"English (US)" = "🇺🇸"
Ukrainian = "🇺🇦"
```

</details>

<details>
<summary>YAML</summary>

```yaml
end:
  - type: keyboard
    show_scroll: false
    icons:
      caps_on: 󰪛
      layout_map:
        "English (US)": 🇺🇸
        Ukrainian: 🇺🇦

```

</details>

<details>
<summary>Corn</summary>

```corn
{
end = [ 
        { 
            type = "keyboard" 
            show_scroll = false 
            icons.caps_on = "󰪛" 
            icons.layout_map.'English (US)' = "🇺🇸"
            icons.layout_map.Ukrainian = "🇺🇦"
        }
    ]
}
```

</details>

## Styling

| Selector                   | Description                                |
| -------------------------- | ------------------------------------------ |
| `.keyboard`                | Keys box container widget.                 |
| `.keyboard .key`           | Individual key indicator container widget. |
| `.keyboard .key.enabled`   | Key indicator where key is toggled on.     |
| `.keyboard .key.caps`      | Capslock key indicator.                    |
| `.keyboard .key.num`       | Num lock key indicator.                    |
| `.keyboard .key.scroll`    | Scroll lock key indicator.                 |
| `.keyboard .key.image`     | Key indicator image icon.                  |
| `.keyboard .key.text-icon` | Key indicator textual icon.                |
| `.keyboard .layout`        | Keyboard layout indicator.                 |

For more information on styling, please see the [styling guide](styling-guide).
