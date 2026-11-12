#!/usr/bin/env bash
# Encode the raw recording (scripts/video/record.mjs) into the page's mp4 and
# the README's GIF (the cheat shot, 10 s); trim.txt cuts the page load before the first caption.
set -euo pipefail
cd "$(dirname "$0")/../.."
raw=$(ls out/video/raw/*.webm | head -1)
mkdir -p web/media
trim=$(cat out/video/trim.txt 2>/dev/null || echo 0)
ffmpeg -v error -y -ss "$trim" -i "$raw" -vf "scale=1920:1080" -c:v libx264 -preset slow -crf 27 -pix_fmt yuv420p -movflags +faststart -an web/media/agent-exit-demo.mp4
ffmpeg -v error -y -ss 28 -t 10 -i "$raw" -vf "fps=8,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96[p];[b][p]paletteuse=dither=bayer" docs/img/demo.gif
node scripts/video/mix.mjs
ls -la web/media/agent-exit-demo.mp4 docs/img/demo.gif
