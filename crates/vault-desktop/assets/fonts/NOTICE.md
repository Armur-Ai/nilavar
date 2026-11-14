# Bundled fonts and icons

All fonts are embedded in the application binary; nothing is downloaded at
runtime.

| File | Font | Version / source | License |
|---|---|---|---|
| `JetBrainsMono-Regular.ttf`, `JetBrainsMono-Bold.ttf` | JetBrains Mono | v2.304, https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304 | SIL Open Font License 1.1 (`JetBrainsMono-OFL.txt`) |
| `Inter-Regular.ttf`, `Inter-SemiBold.ttf` | Inter | v4.1 (`extras/ttf`), https://github.com/rsms/inter/releases/tag/v4.1 | SIL Open Font License 1.1 (`Inter-LICENSE.txt`) |

SHA-256:

```
a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f  JetBrainsMono-Regular.ttf
5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb  JetBrainsMono-Bold.ttf
40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82  Inter-Regular.ttf
78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3  Inter-SemiBold.ttf
```

Icons: Phosphor Icons (regular weight) via the `egui-phosphor` crate, MIT
license, https://github.com/phosphor-icons/core.

Fallback glyphs (emoji and wide Unicode coverage) come from the fonts that
ship with egui (`default_fonts` feature: Ubuntu-Light, Hack, Noto Emoji,
emoji-icon-font), each under its own open license as distributed by egui,
plus system fonts discovered at runtime by eframe for scripts the bundled
fonts do not cover.
