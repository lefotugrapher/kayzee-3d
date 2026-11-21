#!/bin/sh
# Builds Kayzee 3D.app next to the app folder: release binary, icon, Info.plist, ad-hoc signature.
set -e
cd "$(dirname "$0")/.."
cargo build --release
APP="../Kayzee 3D.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/kayzee3d "$APP/Contents/MacOS/kayzee3d"
# the helper that joins rendered frames into a movie
if [ ! -f target/kayzee3d-movie ] || [ tools/moviewriter.swift -nt target/kayzee3d-movie ]; then
  swiftc -O tools/moviewriter.swift -o target/kayzee3d-movie 2>/dev/null
fi
cp target/kayzee3d-movie "$APP/Contents/MacOS/kayzee3d-movie"
if [ ! -f tools/AppIcon.icns ]; then
  T=$(mktemp -d)
  python3 tools/make_icon.py "$T/icon.png"
  mkdir "$T/AppIcon.iconset"
  for s in 16 32 128 256 512; do
    sips -z $s $s "$T/icon.png" --out "$T/AppIcon.iconset/icon_${s}x${s}.png" >/dev/null
    d=$((s * 2))
    sips -z $d $d "$T/icon.png" --out "$T/AppIcon.iconset/icon_${s}x${s}@2x.png" >/dev/null
  done
  iconutil -c icns "$T/AppIcon.iconset" -o tools/AppIcon.icns
  rm -rf "$T"
fi
cp tools/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Kayzee 3D</string>
  <key>CFBundleDisplayName</key><string>Kayzee 3D</string>
  <key>CFBundleIdentifier</key><string>local.kayzee3d.app</string>
  <key>CFBundleExecutable</key><string>kayzee3d</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
</dict>
</plist>
PLIST
codesign --force -s - "$APP/Contents/MacOS/kayzee3d-movie" >/dev/null 2>&1
codesign --force -s - "$APP" >/dev/null 2>&1
echo "built $APP"
