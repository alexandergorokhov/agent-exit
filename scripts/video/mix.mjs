#!/usr/bin/env node
// Mix the narration (out/video/voice-track.json from record.mjs, wavs from
// voice.mjs) into the mp4: each line at the moment its caption appeared.
import { readFileSync, renameSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
const root = new URL("../../", import.meta.url).pathname;
const trackFile = root + "out/video/voice-track.json";
if (!existsSync(trackFile)) { console.log("mix: no voice track (run voice.mjs before record.mjs); the video stays silent"); process.exit(0); }
const track = JSON.parse(readFileSync(trackFile, "utf8"));
const mp4 = root + "web/media/agent-exit-demo.mp4", tmp = root + "out/video/narrated.mp4";
const args = ["-v", "error", "-y", "-i", mp4];
for (const x of track) args.push("-i", root + "out/video/voice/" + x.file);
const parts = track.map((x, i) => `[${i + 1}:a]adelay=${Math.round(x.t * 1000)}:all=1[a${i}]`);
const filter = `${parts.join(";")};${track.map((_, i) => `[a${i}]`).join("")}amix=inputs=${track.length}:normalize=0,loudnorm=I=-16:TP=-1.5[voice]`;
args.push("-filter_complex", filter, "-map", "0:v", "-map", "[voice]", "-c:v", "copy", "-c:a", "aac", "-b:a", "96k", "-shortest", "-movflags", "+faststart", tmp);
execFileSync("ffmpeg", args, { stdio: "inherit" });
renameSync(tmp, mp4);
console.log(`mix: ${track.length} narration lines mixed into web/media/agent-exit-demo.mp4`);
