# Packaging assets

Application icon sources and derived artifacts.

- `grit-icon.svg` — SVG source (the "G" commit-graph mark)
- `grit-icon-1024.png` — master render, Apple Big Sur squircle shape
- `grit.icns` — macOS bundle icon, deployed to `Contents/Resources/grit.icns`
  and referenced by `CFBundleIconFile` in the release workflow
- `grit.png` — 256×256 PNG for the Linux tarball's hicolor icon entry

## Regenerating

macOS does not round icon corners itself — the rounded shape is baked into
every tile. Do it with one script (needs `python3 -m pip install pillow`):

```sh
python3 packaging/make_icon.py
```

It renders `grit-icon.svg` at 1024px via QuickLook, insets the artwork into
the 824×824 squircle mask (~185px radius), then emits the icns tiles and the
Linux PNG.
