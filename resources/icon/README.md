# Shellfox icons

The original `shellfox-icon-source.png` is 1254×1254 RGBA. Its corners are fully transparent. The visible rounded tile occupies bounds 61,61 to 1194,1193. No opaque-background mask or artwork repainting is needed.

Regenerate from the project root with Python 3 and Pillow 11.1.0:

```sh
python3 scripts/generate-icons.py
# Windows with Debian WSL:
wsl -d Debian -- python3 scripts/generate-icons.py
```

The script crops to the alpha >127 bounds, retains the original RGBA edges, and centers the tile at about 90% of a 1024×1024 transparent canvas. It writes `icon.png`, `icon-{16,24,32,48,64,128,256,512}.png`, `icon.ico`, `icon.icns`, and `favicon.ico`. ICO includes all PNG sizes through 256. ICNS uses PNG chunks for standard and Retina representations through 1024.

Forge uses the extensionless `icon` path for native packaging. Squirrel uses the ICO for Setup.exe and Update.exe. Its public metadata URL defaults to this repository's `main` branch and can be overridden with `SHELLFOX_ICON_URL`. The URL will only resolve after the asset is published; local packaging embeds the ICO without downloading it.

The Debian maker installs the PNG under the `shellfox` theme name used by `resources/linux/shellfox.desktop`. The smaller PNGs are also available for Linux hicolor installation. Zip archives inherit the native packaged icon and need no separate maker icon option.

`build.mjs` copies the window ICO and 256px PNG into `tmp/build/icon`, which is staged into app.asar. BrowserWindow resolves them relative to the built main entry, in both development and packaged apps. Vite bundles the renderer's 16px and 32px favicon links.

Validate formats and wiring with `node --test scripts/icons.test.mjs`.
