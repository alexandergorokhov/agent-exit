#!/usr/bin/env node
// Narration for the demo video, made locally with Piper (free, offline TTS):
//   PIPER=/path/to/piper PIPER_MODEL=/path/to/en-us-libritts-high.onnx PIPER_SPEAKER=100 node scripts/video/voice.mjs
// Reads every narration line in script.mjs, writes one wav per line to
// out/video/voice/ and their lengths to out/video/voice/manifest.json. record.mjs
// then holds each caption at least as long as its line is spoken.
import { writeFileSync, mkdirSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { speech } from "./speech.mjs";
import { LINES } from "./script.mjs";

const { PIPER, PIPER_MODEL, PIPER_SPEAKER } = process.env;
if (!PIPER || !PIPER_MODEL) { console.error("voice: set PIPER (the piper binary) and PIPER_MODEL (a voice .onnx)"); process.exit(2); }
const lines = new Set(Object.values(LINES));
const OUT = new URL("../../out/video/voice/", import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });
const manifest = {};
for (const raw of lines) {
  const text = speech(raw);
  const file = `${createHash("sha256").update(text).digest("hex").slice(0, 12)}.wav`;
  if (!existsSync(OUT + file)) execFileSync(PIPER, ["-m", PIPER_MODEL, ...(PIPER_SPEAKER ? ["-s", PIPER_SPEAKER] : []), "-f", OUT + file, "--length_scale", "1.25", "--sentence_silence", "0.35"], { input: text });
  const dur = Number(execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", OUT + file]).toString().trim());
  manifest[text] = { file, dur };
}
writeFileSync(OUT + "manifest.json", JSON.stringify(manifest, null, 2) + "\n");
console.log(`voice: ${Object.keys(manifest).length} lines, ${Object.values(manifest).reduce((a, v) => a + v.dur, 0).toFixed(1)} s spoken -> ${OUT}`);
