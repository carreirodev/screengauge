// PROTOTYPE, one-off: packs the prototype's RGBA frames into .ico files (32-bit DIB entries) for the real-tray check.
// frames.json is what index.html returns from
//   JSON.stringify({ normal: await __icoFrames('normal', sizes), strong: await __icoFrames('strong', sizes) })
// (run in the page's console; WPC-291 used sizes 16, 20, 24, 28, 32, 40, 48, 64, 256).
// Usage: node make-ico.mjs <frames.json> <outDir>
import fs from 'node:fs';
import path from 'node:path';

let data = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
if (typeof data === 'string') data = JSON.parse(data);
const outDir = process.argv[3];

function dib(n, rgba) {
  const maskRow = Math.ceil(n / 32) * 4;
  const buf = Buffer.alloc(40 + n * n * 4 + maskRow * n);
  buf.writeUInt32LE(40, 0);
  buf.writeInt32LE(n, 4);
  buf.writeInt32LE(n * 2, 8);
  buf.writeUInt16LE(1, 12);
  buf.writeUInt16LE(32, 14);
  buf.writeUInt32LE(n * n * 4 + maskRow * n, 20);
  let o = 40;
  for (let y = n - 1; y >= 0; y--) {
    for (let x = 0; x < n; x++) {
      const i = (y * n + x) * 4;
      buf[o++] = rgba[i + 2];
      buf[o++] = rgba[i + 1];
      buf[o++] = rgba[i];
      buf[o++] = rgba[i + 3];
    }
  }
  for (let y = n - 1; y >= 0; y--) {
    for (let x = 0; x < n; x++) {
      if (rgba[(y * n + x) * 4 + 3] === 0) buf[o + (x >> 3)] |= 0x80 >> (x & 7);
    }
    o += maskRow;
  }
  return buf;
}

for (const [variant, frames] of Object.entries(data)) {
  const sizes = Object.keys(frames).map(Number).sort((a, b) => a - b);
  const images = sizes.map(n => dib(n, Buffer.from(frames[n], 'base64')));
  const head = Buffer.alloc(6 + 16 * sizes.length);
  head.writeUInt16LE(0, 0);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(sizes.length, 4);
  let offset = head.length;
  sizes.forEach((n, k) => {
    const e = 6 + 16 * k;
    head[e] = n >= 256 ? 0 : n;
    head[e + 1] = n >= 256 ? 0 : n;
    head.writeUInt16LE(1, e + 4);
    head.writeUInt16LE(32, e + 6);
    head.writeUInt32LE(images[k].length, e + 8);
    head.writeUInt32LE(offset, e + 12);
    offset += images[k].length;
  });
  const file = path.join(outDir, `screengauge-${variant}.ico`);
  fs.writeFileSync(file, Buffer.concat([head, ...images]));
  console.log(file, sizes.join(','), fs.statSync(file).size, 'bytes');
}
