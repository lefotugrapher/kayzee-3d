#!/bin/sh
# Send a message to everyone running Kayzee 3D.
#   tools/news.sh "Title" "One line of text" [https-link] [days-to-show]
# Signs it with your private key (kept outside this repo), adds it to news.json and pushes.
# Apps show it once, the next time they open. A message not signed by your key is ignored.
set -e
[ -n "$2" ] || { echo 'usage: tools/news.sh "Title" "Text" [link] [days]'; exit 2; }
cd "$(dirname "$0")/.."
BIN=target/release/kayzee3d
[ -x "$BIN" ] || cargo build --release
"$BIN" --news-sign news.json "$1" "$2" "${3:-}" "${4:-30}"
git add news.json
git commit -q -m "News: $1"
git push -q origin main
echo "Sent. People see it the next time they open the app."
