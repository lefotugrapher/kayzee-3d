#!/bin/sh
# Publish a new version: builds the Mac app and the Windows program, pushes the
# code, and creates a GitHub release. People running an older copy are told
# about it the next time they open the app.
#
#   tools/release.sh 0.3.0 "What changed in this version"
#
# Needs: Rust, the Xcode command line tools, zig + cargo-zigbuild, and the gh CLI signed in.
set -e
V="$1"
NOTES="${2:-Improvements and fixes.}"
[ -n "$V" ] || { echo "usage: tools/release.sh VERSION \"notes\""; exit 2; }
cd "$(dirname "$0")/.."
perl -0pi -e 's/^version = "[^"]*"/version = "'"$V"'"/m' Cargo.toml
./tools/bundle.sh
cargo zigbuild --release --target x86_64-pc-windows-gnu
OUT=$(mktemp -d)
ditto -c -k --keepParent "../Kayzee 3D.app" "$OUT/Kayzee3D-mac.zip"
mkdir "$OUT/Kayzee3D-Windows"
cp target/x86_64-pc-windows-gnu/release/kayzee3d.exe "$OUT/Kayzee3D-Windows/Kayzee3D.exe"
cp tools/README-windows.txt "$OUT/Kayzee3D-Windows/README.txt"
(cd "$OUT" && zip -q -r Kayzee3D-Windows.zip Kayzee3D-Windows)
git add -A
git commit -q -m "Kayzee 3D $V" || true
git tag "v$V"
git push -q origin main --tags
gh release create "v$V" "$OUT/Kayzee3D-mac.zip" "$OUT/Kayzee3D-Windows.zip" --title "Kayzee 3D $V" --notes "$NOTES"
echo "Released $V"
