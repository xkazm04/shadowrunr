# SPDX-License-Identifier: Apache-2.0
"""A small binary glTF 2.0 (GLB) writer and reader: numpy and the standard library only.

The lich asset is a handful of named nodes (two meshes and four empties) and one texture, so the
writer covers exactly that: triangle meshes with positions, normals, UVs and one base-colour image
(PNG or JPEG) each, plus an optional tangent-space normal map, and nodes with a name, a translation and an optional mesh. The reader returns the parsed
JSON, the binary chunk and accessor arrays, which is all ``--check`` and the tests need.
"""

from __future__ import annotations

import json
import struct
from dataclasses import dataclass, field

import numpy as np

MAGIC = 0x46546C67  # "glTF"
CHUNK_JSON = 0x4E4F534A
CHUNK_BIN = 0x004E4942

FLOAT = 5126
U16 = 5123
U32 = 5125
ARRAY_BUFFER = 34962
ELEMENT_ARRAY_BUFFER = 34963

_NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
_DTYPE = {FLOAT: np.float32, U16: np.uint16, U32: np.uint32, 5121: np.uint8}


@dataclass
class Mesh:
    """One triangle mesh in its node's local space."""

    positions: np.ndarray  # (n, 3) float32
    faces: np.ndarray  # (m, 3) int
    normals: np.ndarray | None = None  # (n, 3)
    uvs: np.ndarray | None = None  # (n, 2), glTF convention: v grows downward
    texture_png: bytes | None = None  # the base-colour image's bytes (PNG, or JPEG: see texture_mime)
    material: str = "bone"
    texture_mime: str = "image/png"
    normal_texture: bytes | None = None  # optional tangent-space normal map (glTF normalTexture)
    normal_mime: str = "image/png"


@dataclass
class Node:
    name: str
    translation: tuple[float, float, float] = (0.0, 0.0, 0.0)
    mesh: Mesh | None = None
    extras: dict = field(default_factory=dict)


def _pad(b: bytes, fill: bytes = b"\x00") -> bytes:
    return b + fill * ((4 - len(b) % 4) % 4)


class _Bin:
    def __init__(self) -> None:
        self.data = bytearray()
        self.views: list[dict] = []
        self.accessors: list[dict] = []

    def view(self, raw: bytes, target: int | None = None) -> int:
        while len(self.data) % 4:
            self.data.append(0)
        v = {"buffer": 0, "byteOffset": len(self.data), "byteLength": len(raw)}
        if target is not None:
            v["target"] = target
        self.data.extend(raw)
        self.views.append(v)
        return len(self.views) - 1

    def accessor(self, arr: np.ndarray, kind: str, ctype: int, target: int, minmax: bool) -> int:
        arr = np.ascontiguousarray(arr, dtype=_DTYPE[ctype])
        a = {
            "bufferView": self.view(arr.tobytes(), target),
            "componentType": ctype,
            "count": int(arr.shape[0]),
            "type": kind,
        }
        if minmax:
            a["min"] = [float(x) for x in np.atleast_1d(arr.min(axis=0))]
            a["max"] = [float(x) for x in np.atleast_1d(arr.max(axis=0))]
        self.accessors.append(a)
        return len(self.accessors) - 1


def _texture(b: "_Bin", images: list, textures: list, tex_of: dict, raw: bytes, mime: str) -> int:
    if raw not in tex_of:
        images.append({"bufferView": b.view(raw), "mimeType": mime})
        textures.append({"source": len(images) - 1, "sampler": 0})
        tex_of[raw] = len(textures) - 1
    return tex_of[raw]


def write_glb(nodes: list[Node], generator: str = "shadowrunr-avatar3d") -> bytes:
    """Every node is a root node of scene 0, in the order given."""
    b = _Bin()
    gl: dict = {
        "asset": {"version": "2.0", "generator": generator},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": [],
    }
    meshes, materials, textures, images = [], [], [], []
    tex_of: dict[bytes, int] = {}
    for n in nodes:
        jn: dict = {"name": n.name}
        if any(abs(c) > 0 for c in n.translation):
            jn["translation"] = [float(c) for c in n.translation]
        if n.extras:
            jn["extras"] = n.extras
        if n.mesh is not None:
            m = n.mesh
            pos = np.asarray(m.positions, np.float32)
            faces = np.asarray(m.faces).reshape(-1)
            ctype = U16 if pos.shape[0] < 65536 else U32
            attrs = {"POSITION": b.accessor(pos, "VEC3", FLOAT, ARRAY_BUFFER, True)}
            if m.normals is not None:
                attrs["NORMAL"] = b.accessor(m.normals, "VEC3", FLOAT, ARRAY_BUFFER, False)
            if m.uvs is not None:
                attrs["TEXCOORD_0"] = b.accessor(m.uvs, "VEC2", FLOAT, ARRAY_BUFFER, False)
            prim = {
                "attributes": attrs,
                "indices": b.accessor(faces, "SCALAR", ctype, ELEMENT_ARRAY_BUFFER, True),
                "mode": 4,
            }
            mat: dict = {
                "name": m.material,
                "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 1.0},
            }
            if m.texture_png is not None and m.uvs is not None:
                # Meshes that share an atlas share one image and one texture.
                mat["pbrMetallicRoughness"]["baseColorTexture"] = {
                    "index": _texture(b, images, textures, tex_of, m.texture_png, m.texture_mime)}
                if m.normal_texture is not None:
                    mat["normalTexture"] = {
                        "index": _texture(b, images, textures, tex_of, m.normal_texture, m.normal_mime)}
            materials.append(mat)
            prim["material"] = len(materials) - 1
            meshes.append({"name": n.name, "primitives": [prim]})
            jn["mesh"] = len(meshes) - 1
        gl["nodes"].append(jn)
    if meshes:
        gl["meshes"] = meshes
        gl["materials"] = materials
    if textures:
        gl["textures"] = textures
        gl["images"] = images
        # LINEAR_MIPMAP_LINEAR / LINEAR, clamp: a baked atlas must not wrap at its seams.
        gl["samplers"] = [{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}]
    gl["accessors"] = b.accessors
    gl["bufferViews"] = b.views
    binary = _pad(bytes(b.data))
    gl["buffers"] = [{"byteLength": len(binary)}]
    js = _pad(json.dumps(gl, separators=(",", ":")).encode("utf-8"), b" ")
    total = 12 + 8 + len(js) + 8 + len(binary)
    return b"".join(
        [
            struct.pack("<III", MAGIC, 2, total),
            struct.pack("<II", len(js), CHUNK_JSON),
            js,
            struct.pack("<II", len(binary), CHUNK_BIN),
            binary,
        ]
    )


class GlbError(ValueError):
    pass


@dataclass
class Glb:
    json: dict
    bin: bytes

    def accessor(self, index: int) -> np.ndarray:
        a = self.json["accessors"][index]
        v = self.json["bufferViews"][a["bufferView"]]
        dt = np.dtype(_DTYPE[a["componentType"]])
        ncomp = _NCOMP[a["type"]]
        off = v.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = v.get("byteStride", 0) or dt.itemsize * ncomp
        count = a["count"]
        if stride == dt.itemsize * ncomp:
            arr = np.frombuffer(self.bin, dt, count * ncomp, off)
        else:
            rows = [np.frombuffer(self.bin, dt, ncomp, off + i * stride) for i in range(count)]
            arr = np.concatenate(rows) if rows else np.zeros(0, dt)
        return arr.reshape(count, ncomp) if ncomp > 1 else arr.copy()

    def nodes_by_name(self) -> dict[str, dict]:
        return {n.get("name", ""): n for n in self.json.get("nodes", [])}

    def mesh_arrays(self, mesh_index: int) -> tuple[np.ndarray, np.ndarray]:
        """Positions and triangles of every primitive of a mesh, concatenated."""
        pos_all, tri_all, base = [], [], 0
        for p in self.json["meshes"][mesh_index]["primitives"]:
            if p.get("mode", 4) != 4:
                raise GlbError(f"mesh {mesh_index}: only triangle primitives are supported")
            pos = self.accessor(p["attributes"]["POSITION"]).astype(np.float64)
            if "indices" in p:
                idx = self.accessor(p["indices"]).astype(np.int64).reshape(-1, 3)
            else:
                idx = np.arange(pos.shape[0]).reshape(-1, 3)
            pos_all.append(pos)
            tri_all.append(idx + base)
            base += pos.shape[0]
        return np.concatenate(pos_all), np.concatenate(tri_all)


def read_glb(data: bytes) -> Glb:
    if len(data) < 20:
        raise GlbError("too short to be a GLB")
    magic, version, total = struct.unpack_from("<III", data, 0)
    if magic != MAGIC:
        raise GlbError("not a GLB (bad magic)")
    if version != 2:
        raise GlbError(f"glTF version {version}, expected 2")
    if total != len(data):
        raise GlbError(f"header length {total} but file is {len(data)} bytes")
    off, js, binary = 12, None, b""
    while off < len(data):
        clen, ctype = struct.unpack_from("<II", data, off)
        chunk = data[off + 8 : off + 8 + clen]
        if ctype == CHUNK_JSON:
            js = json.loads(chunk.decode("utf-8"))
        elif ctype == CHUNK_BIN:
            binary = bytes(chunk)
        off += 8 + clen
    if js is None:
        raise GlbError("no JSON chunk")
    return Glb(js, binary)
