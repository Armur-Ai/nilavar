# Bundled fonts and icons

All fonts are embedded in the application binary; nothing is downloaded at
runtime.

| File | Font | Version / source | License |
|---|---|---|---|
| `JetBrainsMono-Regular.ttf`, `JetBrainsMono-Bold.ttf` | JetBrains Mono | v2.304, https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304 | SIL Open Font License 1.1 (`JetBrainsMono-OFL.txt`) |
| `Inter-Regular.ttf`, `Inter-SemiBold.ttf` | Inter | v4.1 (`extras/ttf`), https://github.com/rsms/inter/releases/tag/v4.1 | SIL Open Font License 1.1 (`Inter-LICENSE.txt`) |

**Modification:** the four text-font files have had their Private Use Area
(U+E000–U+F8FF) `cmap` entries removed with `tools/strip_pua.py` so they do
not shadow the icon font. Glyph outlines are unchanged. Neither font declares
a Reserved Font Name, so modified redistribution under the OFL is permitted.
SHA-256 of the original upstream files:

```
a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f  JetBrainsMono-Regular.ttf
5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb  JetBrainsMono-Bold.ttf
40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82  Inter-Regular.ttf
78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3  Inter-SemiBold.ttf
```

SHA-256 of the bundled (modified) files:

```
54d37a6f26d430faad69958368190049e163e15bcfacdba76ff6ae53677c2703  JetBrainsMono-Regular.ttf
0d1916a69e8157514dfaaebb93dcd9967bcea3920071872b6a6681359d8ec068  JetBrainsMono-Bold.ttf
2020943e1dbc41a419a309581e6ab4c59472b357247553349a6343181cb129ff  Inter-Regular.ttf
b90216f9d5a87d73c9625ba7187afb40a04c60d4bdf1cd8cd64a58f951985b9c  Inter-SemiBold.ttf
06b91e022b7ee899a63efced879392a74f0bacbda54e4467e9f663220d173a10  Phosphor-Regular.ttf
```

Icons: `Phosphor-Regular.ttf` — Phosphor Icons (regular weight), MIT license
(`Phosphor-LICENSE-MIT.txt`), https://github.com/phosphor-icons/core; file taken
from the egui-phosphor 0.14.0 crate.

Fallback glyphs (emoji and wide Unicode coverage) come from the fonts that
ship with egui (`default_fonts` feature: Ubuntu-Light, Hack, Noto Emoji,
emoji-icon-font), each under its own open license as distributed by egui,
plus system fonts discovered at runtime by eframe for scripts the bundled
fonts do not cover.
