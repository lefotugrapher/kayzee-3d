#!/bin/sh
# Builds Kayzee 3D.app next to the app folder: release binary, icon, Info.plist, ad-hoc signature.
set -e
cd "$(dirname "$0")/.."
# one program for both kinds of Mac: Apple Silicon and Intel
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
mkdir -p target/universal
lipo -create target/aarch64-apple-darwin/release/kayzee3d target/x86_64-apple-darwin/release/kayzee3d -output target/universal/kayzee3d
cp target/universal/kayzee3d target/release/kayzee3d 2>/dev/null || true
APP="../Kayzee 3D.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/universal/kayzee3d "$APP/Contents/MacOS/kayzee3d"
# the helper that joins rendered frames into a movie
if [ ! -f target/kayzee3d-movie ] || [ tools/moviewriter.swift -nt target/kayzee3d-movie ] || ! lipo -archs target/kayzee3d-movie | grep -q x86_64; then
  swiftc -O -target arm64-apple-macos12 tools/moviewriter.swift -o target/kayzee3d-movie-arm64 2>/dev/null
  swiftc -O -target x86_64-apple-macos12 tools/moviewriter.swift -o target/kayzee3d-movie-x86_64 2>/dev/null
  lipo -create target/kayzee3d-movie-arm64 target/kayzee3d-movie-x86_64 -output target/kayzee3d-movie
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
