# Packaging assets

Application icon sources and derived artifacts.

- `grit-icon.svg` — SVG source (the "G" commit-graph mark)
- `grit.icns` — macOS bundle icon, deployed to `Contents/Resources/grit.icns`
  and referenced by `CFBundleIconFile` in the release workflow
- `grit.png` — 256×256 PNG for the Linux tarball's hicolor icon entry
- `grit-icon-1024.png` — master render used to derive every size

## Regenerating the derived files

```sh
# 1024px master render (QuickLook renders SVG on macOS)
qlmanage -t -s 1024 -o . packaging/grit-icon.svg
cp grit-icon.svg.png src.png

# every icns tile
mkdir -p icon.iconset
for spec in "16 icon_16x16.png" "32 icon_16x16@2x.png" "32 icon_32x32.png" \
            "64 icon_32x32@2x.png" "128 icon_128x128.png" "256 icon_128x128@2x.png" \
            "256 icon_256x256.png" "512 icon_256x256@2x.png" \
            "512 icon_512x512.png" "1024 icon_512x512@2x.png"; do
    size=${spec%% *}; name=${spec#* }
    sips -s format png -z $size $size src.png --out icon.iconset/$name >/dev/null
done

# .icns + 256px Linux PNG
iconutil -c icns icon.iconset -o grit.icns
sips -z 256 256 src.png --out grit.png >/dev/null
```
