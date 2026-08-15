# Emoji art

The PNGs in this directory are **Twemoji**, © Twitter/X and contributors,
licensed **CC-BY 4.0**. Source: <https://github.com/jdecked/twemoji> (the
community-maintained continuation), 72×72 raster set.

CC-BY asks for credit, which this file provides. If the project ever ships
publicly, surface it somewhere a user can find — an about screen or a credits
file — rather than leaving it only in the asset tree.

## Adding a new emoji

Drop a 72×72 PNG here named for how you want to reference it. The loader keys
on the **filename stem**, so `skull.png` is reachable as `{emoji:skull}` in
`assets/config/banter.ron`. No code change, no rebuild — the directory is
scanned at startup.

To pull another Twemoji, find its codepoint (e.g. 🧊 is `1f9ca`) and:

```bash
curl -o assets/icons/emoji/icecube.png \
  https://cdn.jsdelivr.net/gh/jdecked/twemoji@latest/assets/72x72/1f9ca.png
```

Twemoji filenames omit the `FE0F` variation selector, so ✔️ (`2714 FE0F`) is
just `2714.png`.

Any other art works too — these are ordinary textures, not a font. The emoji
set is here because it is broad, familiar, and consistent, not because the
renderer requires it.
