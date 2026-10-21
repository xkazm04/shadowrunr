// A grayscale PNG decoder for the depth map. A browser decodes a PNG to 8 bits a channel, and
// the depth map is 16-bit (`tools/avatar/depth_map.py`), so the face reads the file itself:
// the chunks, the zlib stream (the platform's `DecompressionStream`) and the row filters.
// Grayscale only (colour type 0), 8 or 16 bits, not interlaced: what the prep writes.

/** Inflate a zlib stream. The default is the platform's; tests may pass node's. */
export type Inflate = (data: Uint8Array) => Promise<Uint8Array>;

export async function inflateZlib(data: Uint8Array): Promise<Uint8Array> {
  const stream = new Blob([data as BlobPart]).stream().pipeThrough(new DecompressionStream("deflate"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

export interface Gray16 {
  width: number;
  height: number;
  /** Row-major samples, 0..65535 (an 8-bit file is scaled up by 257). */
  data: Uint16Array;
}

const SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10];

const paeth = (a: number, b: number, c: number) => {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
};

/** Decode a grayscale PNG. Rejects, with the reason, anything else. */
export async function decodeGrayPng(bytes: Uint8Array, inflate: Inflate = inflateZlib): Promise<Gray16> {
  if (bytes.length < 33 || SIGNATURE.some((b, i) => bytes[i] !== b)) throw new Error("not a PNG");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 8;
  let width = 0;
  let height = 0;
  let depth = 0;
  const idat: Uint8Array[] = [];
  while (at + 8 <= bytes.length) {
    const len = view.getUint32(at);
    const type = String.fromCharCode(bytes[at + 4]!, bytes[at + 5]!, bytes[at + 6]!, bytes[at + 7]!);
    const body = bytes.subarray(at + 8, at + 8 + len);
    if (body.length !== len) throw new Error("the PNG is cut short");
    if (type === "IHDR") {
      width = view.getUint32(at + 8);
      height = view.getUint32(at + 12);
      depth = body[8]!;
      const colour = body[9]!;
      const interlace = body[12]!;
      if (colour !== 0) throw new Error(`the PNG is colour type ${colour}, not grayscale`);
      if (depth !== 16 && depth !== 8) throw new Error(`the PNG is ${depth}-bit, not 8 or 16`);
      if (interlace !== 0) throw new Error("the PNG is interlaced");
    } else if (type === "IDAT") idat.push(body);
    else if (type === "IEND") break;
    at += 12 + len;
  }
  if (!width || !height) throw new Error("the PNG has no header");
  const packed = new Uint8Array(idat.reduce((n, c) => n + c.length, 0));
  let k = 0;
  for (const c of idat) {
    packed.set(c, k);
    k += c.length;
  }
  const raw = await inflate(packed);
  const bpp = depth / 8;
  const stride = width * bpp;
  if (raw.length < height * (stride + 1)) throw new Error("the PNG's image data is short");

  // undo the row filters in place, into `rows`
  const rows = new Uint8Array(height * stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)]!;
    const src = y * (stride + 1) + 1;
    const out = y * stride;
    const prev = out - stride;
    for (let i = 0; i < stride; i++) {
      const x = raw[src + i]!;
      const a = i >= bpp ? rows[out + i - bpp]! : 0;
      const b = y > 0 ? rows[prev + i]! : 0;
      const c = i >= bpp && y > 0 ? rows[prev + i - bpp]! : 0;
      let v: number;
      switch (filter) {
        case 0:
          v = x;
          break;
        case 1:
          v = x + a;
          break;
        case 2:
          v = x + b;
          break;
        case 3:
          v = x + ((a + b) >> 1);
          break;
        case 4:
          v = x + paeth(a, b, c);
          break;
        default:
          throw new Error(`the PNG has an unknown row filter ${filter}`);
      }
      rows[out + i] = v & 0xff;
    }
  }

  const data = new Uint16Array(width * height);
  if (depth === 16) for (let i = 0; i < data.length; i++) data[i] = (rows[2 * i]! << 8) | rows[2 * i + 1]!;
  else for (let i = 0; i < data.length; i++) data[i] = rows[i]! * 257;
  return { width, height, data };
}
