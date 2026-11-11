#!/usr/bin/env bash
# Encode the raw recording (scripts/video/record.mjs) into the page's mp4 and
# the README's GIF (the due-diligence shot, 10 s).
set -euo pipefail
cd "$(dirname "$0")/../.."
raw=$(ls out/video/raw/*.webm | head -1)
mkdir -p web/media
ffmpeg -v error -y -i "$raw" -c:v libx264 -preset slow -crf 30 -pix_fmt yuv420p -movflags +faststart -an web/media/agent-exit-demo.mp4
ffmpeg -v error -y -ss 13 -t 10 -i "$raw" -vf "fps=8,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96[p];[b][p]paletteuse=dither=bayer" docs/img/demo.gif
node scripts/video/mix.mjs
ls -la web/media/agent-exit-demo.mp4 docs/img/demo.gif
