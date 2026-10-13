// Renders app-icon.svg to every icon format Tauri needs (PNG, ICO, ICNS).
// Usage: node scripts/make-icons.mjs   (needs the "sharp" package)
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
let sharp;
try { sharp = require("sharp"); } catch { sharp = require(process.env.SHARP_PATH); }

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const svg = fs.readFileSync(path.join(root, "src-tauri/app-icon.svg"));
const out = path.join(root, "src-tauri/icons");
fs.mkdirSync(out, { recursive: true });

const png = (size) => sharp(svg, { density: Math.max(72, (72 * size) / 512 * 2) }).resize(size, size).ensureAlpha().png().toBuffer();

const files = { "32x32.png": 32, "128x128.png": 128, "128x128@2x.png": 256, "icon.png": 512 };
for (const [name, size] of Object.entries(files)) fs.writeFileSync(path.join(out, name), await png(size));

// ICO with embedded PNGs
const icoSizes = [16, 24, 32, 48, 64, 128, 256];
const icoImgs = await Promise.all(icoSizes.map(png));
const header = Buffer.alloc(6); header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(icoSizes.length, 4);
let offset = 6 + 16 * icoSizes.length;
const dir = icoSizes.map((s, i) => {
  const e = Buffer.alloc(16);
  e.writeUInt8(s >= 256 ? 0 : s, 0); e.writeUInt8(s >= 256 ? 0 : s, 1);
  e.writeUInt16LE(1, 4); e.writeUInt16LE(32, 6);
  e.writeUInt32LE(icoImgs[i].length, 8); e.writeUInt32LE(offset, 12);
  offset += icoImgs[i].length; return e;
});
fs.writeFileSync(path.join(out, "icon.ico"), Buffer.concat([header, ...dir, ...icoImgs]));

// ICNS with PNG chunks
const icnsTypes = [["icp4", 16], ["icp5", 32], ["icp6", 64], ["ic07", 128], ["ic08", 256], ["ic09", 512], ["ic10", 1024]];
const chunks = [];
for (const [type, size] of icnsTypes) {
  const data = await png(size);
  const h = Buffer.alloc(8); h.write(type, 0, "ascii"); h.writeUInt32BE(data.length + 8, 4);
  chunks.push(h, data);
}
const body = Buffer.concat(chunks);
const ih = Buffer.alloc(8); ih.write("icns", 0, "ascii"); ih.writeUInt32BE(body.length + 8, 4);
fs.writeFileSync(path.join(out, "icon.icns"), Buffer.concat([ih, body]));
console.log("icons written to", out);
