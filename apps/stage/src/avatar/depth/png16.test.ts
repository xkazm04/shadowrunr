import { deflateSync, inflateSync } from "node:zlib";

import { describe, expect, it } from "vitest";

import { decodeGrayPng, inflateZlib } from "./png16";

function chunk(type: string, body: Uint8Array): Buffer {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(body.length, 0);
  head.write(type, 4, "ascii");
  return Buffer.concat([head, Buffer.from(body), Buffer.alloc(4)]); // the CRC is not checked
}

/** A grayscale PNG, each row with the given filter applied (as an encoder would). */
function png(width: number, height: number, samples: number[], depth: 8 | 16, filters: number[], colour = 0): Uint8Array {
  const bpp = depth / 8;
  const stride = width * bpp;
  const rows: number[][] = [];
  for (let y = 0; y < height; y++) {
    const row: number[] = [];
    for (let x = 0; x < width; x++) {
      const v = samples[y * width + x]!;
      if (depth === 16) row.push(v >> 8, v & 255);
      else row.push(v);
    }
    rows.push(row);
  }
  const raw: number[] = [];
  for (let y = 0; y < height; y++) {
    const f = filters[y % filters.length]!;
    raw.push(f);
    for (let i = 0; i < stride; i++) {
      const x = rows[y]![i]!;
      const a = i >= bpp ? rows[y]![i - bpp]! : 0;
      const b = y > 0 ? rows[y - 1]![i]! : 0;
      const c = i >= bpp && y > 0 ? rows[y - 1]![i - bpp]! : 0;
      const p = a + b - c;
      const pr = Math.abs(p - a) <= Math.abs(p - b) && Math.abs(p - a) <= Math.abs(p - c) ? a : Math.abs(p - b) <= Math.abs(p - c) ? b : c;
      const pred = [0, a, b, (a + b) >> 1, pr][f]!;
      raw.push((x - pred) & 255);
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = depth;
  ihdr[9] = colour;
  const data = deflateSync(Buffer.from(raw));
  // split the image data over two IDAT chunks, as encoders may
  const half = Math.floor(data.length / 2);
  return new Uint8Array(
    Buffer.concat([
      Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
      chunk("IHDR", ihdr),
      chunk("IDAT", data.subarray(0, half)),
      chunk("IDAT", data.subarray(half)),
      chunk("IEND", new Uint8Array(0)),
    ]),
  );
}

const nodeInflate = async (d: Uint8Array) => new Uint8Array(inflateSync(d));

describe("the depth map's PNG decoder", () => {
  const W = 7;
  const H = 6;
  const samples = Array.from({ length: W * H }, (_, i) => (i * 7919 + 13) % 65536);

  it("reads 16-bit grayscale through every row filter", async () => {
    const out = await decodeGrayPng(png(W, H, samples, 16, [0, 1, 2, 3, 4, 4]), nodeInflate);
    expect(out.width).toBe(W);
    expect(out.height).toBe(H);
    expect(Array.from(out.data)).toEqual(samples);
  });

  it("scales 8-bit grayscale to the 16-bit range", async () => {
    const eight = samples.map((s) => s >> 8);
    const out = await decodeGrayPng(png(W, H, eight, 8, [4, 3, 2, 1, 0]), nodeInflate);
    expect(Array.from(out.data)).toEqual(eight.map((s) => s * 257));
  });

  it("inflates with the platform's stream by default", async () => {
    const out = await decodeGrayPng(png(W, H, samples, 16, [2]));
    expect(Array.from(out.data)).toEqual(samples);
    expect(Array.from(await inflateZlib(new Uint8Array(deflateSync(Buffer.from([1, 2, 3])))))).toEqual([1, 2, 3]);
  });

  it("refuses what is not a grayscale PNG", async () => {
    await expect(decodeGrayPng(new Uint8Array(40), nodeInflate)).rejects.toThrow(/not a PNG/);
    await expect(decodeGrayPng(png(2, 2, [1, 2, 3, 4], 8, [0], 2), nodeInflate)).rejects.toThrow(/colour type 2/);
  });
});
