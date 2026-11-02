// Regenerates docs/img/demo.png (the README screenshot): node web/test/screenshot.mjs
// Needs the playwright package with Chromium.
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { serve } from "../serve.mjs";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");
const dir = fileURLToPath(new URL("../../docs/img/", import.meta.url));
mkdirSync(dir, { recursive: true });
const server = await serve(0);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1100, height: 760 } });
await page.goto(`http://127.0.0.1:${server.address().port}/`);
await page.waitForSelector("#step h2");
await page.screenshot({ path: `${dir}demo.png` });
await browser.close();
server.close();
console.log(`wrote ${dir}demo.png`);
