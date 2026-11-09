# Packaging assets

Application icon sources and derived artifacts.

- `grit-icon.svg` — SVG source (the "G" commit-graph mark)
- `grit-icon-1024.png` — master render, Apple Big Sur squircle shape
- `grit.icns` — macOS bundle icon, deployed to `Contents/Resources/grit.icns`
  and referenced by `CFBundleIconFile` in the release workflow
- `grit.ico` — multi-size Windows icon, embedded into `grit.exe` by
  `crates/grit/build.rs` (Explorer, taskbar, Alt-Tab)
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

## Windows installer

`packaging.iss` (repository root) builds the per-user Inno Setup 6 installer.
After `cargo build --release -p grit`, from the repository root:

```powershell
ISCC.exe /DMyAppVersion=0.10.0 packaging.iss
```

Output: `dist/grit-<version>-x64-windows-setup.exe`. It installs under
`%LOCALAPPDATA%\Programs\Grit` without UAC, adds the Start menu entry with
AUMID `com.zareix.grit`, and leaves `%APPDATA%\grit` (settings + session) in
place on uninstall. Sign `grit.exe` before ISCC and the setup afterwards when
a code-signing certificate is available.
