# SPDX-License-Identifier: Apache-2.0
"""An offscreen OpenGL renderer (moderngl) for review renders, the camera fit and texture baking.

Three passes share one context:

- ``draw``: a mesh with per-vertex colour or a UV texture, faceted shading (normals from screen
  derivatives, which is what the low-poly portrait reads as), on black. Returns RGB and the
  coverage mask.
- ``mask``: coverage only, for silhouette fitting.
- ``bake_positions``: rasterise a mesh in its UV space and write the 3D position (and the face
  index) under every texel, which is what the texture bake samples from.

Matrices are row-major numpy and transposed on upload. The camera looks down -Z of its own frame
(OpenGL); the model faces +Z, so a camera at +Z sees its face.
"""

from __future__ import annotations

import math

import numpy as np

_VS = """
#version 330
uniform mat4 mvp;
in vec3 in_pos;
in vec3 in_col;
in vec2 in_uv;
in vec3 in_nrm;
out vec3 v_pos;
out vec3 v_col;
out vec2 v_uv;
out vec3 v_nrm;
void main() {
    v_pos = in_pos;
    v_col = in_col;
    v_uv = in_uv;
    v_nrm = in_nrm;
    gl_Position = mvp * vec4(in_pos, 1.0);
}
"""

_FS = """
#version 330
uniform vec3 light;
uniform vec3 eye;
uniform float lit;
uniform int use_tex;
uniform int use_nrm;
uniform sampler2D tex;
uniform sampler2D nmap;
in vec3 v_pos;
in vec3 v_col;
in vec2 v_uv;
in vec3 v_nrm;
out vec4 frag;
void main() {
    vec3 n = normalize(cross(dFdx(v_pos), dFdy(v_pos)));
    if (dot(n, eye - v_pos) < 0.0) n = -n;
    if (use_nrm == 1) {
        // The smooth normal, bent by the tangent-space map in the frame the UVs imply (the one
        // three.js rebuilds for a mesh without tangents).
        vec3 N = normalize(v_nrm);
        if (dot(N, eye - v_pos) < 0.0) N = -N;
        vec3 dp1 = dFdx(v_pos), dp2 = dFdy(v_pos);
        vec2 duv1 = dFdx(v_uv), duv2 = dFdy(v_uv);
        vec3 dp2perp = cross(dp2, N), dp1perp = cross(N, dp1);
        vec3 T = dp2perp * duv1.x + dp1perp * duv2.x;
        vec3 B = dp2perp * duv1.y + dp1perp * duv2.y;
        float inv = inversesqrt(max(max(dot(T, T), dot(B, B)), 1e-20));
        vec3 nm = texture(nmap, v_uv).xyz * 2.0 - 1.0;
        n = normalize(mat3(T * inv, B * inv, N) * nm);
    }
    vec3 albedo = use_tex == 1 ? texture(tex, v_uv).rgb : v_col;
    float d = max(dot(n, normalize(light)), 0.0);
    float h = max(dot(n, normalize(eye - v_pos)), 0.0);
    float shade = mix(1.0, 0.45 + 0.35 * d + 0.3 * h, lit);
    frag = vec4(albedo * shade, 1.0);
}
"""

_POS_VS = """
#version 330
uniform mat4 mvp;
in vec3 in_pos;
out vec3 v_pos;
void main() {
    v_pos = in_pos;
    gl_Position = mvp * vec4(in_pos, 1.0);
}
"""

_POS_FS = """
#version 330
in vec3 v_pos;
out vec4 frag;
void main() { frag = vec4(v_pos, 1.0); }
"""

_BAKE_VS = """
#version 330
in vec3 in_pos;
in vec2 in_uv;
in float in_face;
out vec3 v_pos;
flat out float v_face;
void main() {
    v_pos = in_pos;
    v_face = in_face;
    gl_Position = vec4(in_uv.x * 2.0 - 1.0, in_uv.y * 2.0 - 1.0, 0.0, 1.0);
}
"""

_BAKE_FS = """
#version 330
in vec3 v_pos;
flat in float v_face;
out vec4 frag;
void main() { frag = vec4(v_pos, v_face + 1.0); }
"""


def look_at(eye, target, up=(0.0, 1.0, 0.0)) -> np.ndarray:
    eye, target, up = (np.asarray(x, np.float64) for x in (eye, target, up))
    f = target - eye
    f /= np.linalg.norm(f)
    s = np.cross(f, up)
    s /= np.linalg.norm(s)
    u = np.cross(s, f)
    m = np.eye(4)
    m[0, :3], m[1, :3], m[2, :3] = s, u, -f
    m[:3, 3] = -m[:3, :3] @ eye
    return m


def perspective(fov_deg: float, aspect: float = 1.0, near: float = 0.05, far: float = 50.0) -> np.ndarray:
    t = 1.0 / math.tan(math.radians(fov_deg) / 2)
    m = np.zeros((4, 4))
    m[0, 0], m[1, 1] = t / aspect, t
    m[2, 2] = (far + near) / (near - far)
    m[2, 3] = 2 * far * near / (near - far)
    m[3, 2] = -1.0
    return m


def project(points: np.ndarray, view: np.ndarray, proj: np.ndarray, size: int) -> np.ndarray:
    """Model points to pixel coordinates (x right, y down) and view depth (positive ahead)."""
    p = np.c_[points, np.ones(len(points))]
    cam = p @ view.T
    clip = cam @ proj.T
    ndc = clip[:, :3] / clip[:, 3:4]
    px = (ndc[:, 0] + 1) / 2 * size
    py = (1 - ndc[:, 1]) / 2 * size
    return np.c_[px, py, -cam[:, 2]]


def unproject_ray(px: float, py: float, view: np.ndarray, proj: np.ndarray, size: int):
    """The ray (origin, unit direction) in model space through a pixel."""
    ndc = np.array([px / size * 2 - 1, 1 - py / size * 2])
    inv = np.linalg.inv(proj @ view)
    a = inv @ np.array([ndc[0], ndc[1], -1.0, 1.0])
    b = inv @ np.array([ndc[0], ndc[1], 1.0, 1.0])
    a, b = a[:3] / a[3], b[:3] / b[3]
    d = b - a
    return a, d / np.linalg.norm(d)


class Renderer:
    def __init__(self, size: int = 1024) -> None:
        import moderngl

        self.mgl = moderngl
        self.size = size
        self.ctx = moderngl.create_standalone_context()
        self.ctx.enable(moderngl.DEPTH_TEST)
        self.prog = self.ctx.program(vertex_shader=_VS, fragment_shader=_FS)
        self.bake_prog = self.ctx.program(vertex_shader=_BAKE_VS, fragment_shader=_BAKE_FS)
        self.pos_prog = self.ctx.program(vertex_shader=_POS_VS, fragment_shader=_POS_FS)
        self._fbos: dict = {}

    def _fbo(self, w: int, h: int, dtype: str = "f1", samples: int = 0):
        key = (w, h, dtype, samples)
        if key not in self._fbos:
            col = self.ctx.renderbuffer((w, h), 4, dtype=dtype, samples=samples)
            dep = self.ctx.depth_renderbuffer((w, h), samples=samples)
            self._fbos[key] = self.ctx.framebuffer(col, dep)
        return self._fbos[key]

    def _vao(self, prog, v, f, col=None, uv=None, nrm=None):
        tri = np.asarray(v, np.float32)[np.asarray(f).reshape(-1)]
        n = len(tri)
        c = np.ones((n, 3), np.float32) if col is None else np.asarray(col, np.float32)[np.asarray(f).reshape(-1)]
        t = np.zeros((n, 2), np.float32) if uv is None else np.asarray(uv, np.float32)[np.asarray(f).reshape(-1)]
        nr = np.zeros((n, 3), np.float32) if nrm is None else np.asarray(nrm, np.float32)[np.asarray(f).reshape(-1)]
        buf = self.ctx.buffer(np.hstack([tri, c, t, nr]).astype("f4").tobytes())
        return self.ctx.vertex_array(prog, [(buf, "3f 3f 2f 3f", "in_pos", "in_col", "in_uv", "in_nrm")]), buf

    def draw(self, meshes, view, proj, eye, light=(-0.6, 0.8, 0.9), lit: float = 1.0,
             ssaa: int = 2, bg=(0.0, 0.0, 0.0)) -> tuple[np.ndarray, np.ndarray]:
        """``meshes``: dicts with ``v``, ``f`` and either ``c`` (per vertex) or ``uv`` + ``tex``
        (an (h, w, 3) uint8 image, row 0 at the top as glTF stores it); a mesh with ``nmap`` (a
        tangent-space normal image) and ``n`` (smooth vertex normals) is shaded through the map.
        Returns RGB uint8 and the
        coverage mask, both ``size`` square."""
        s = self.size * ssaa
        fbo = self._fbo(s, s)
        fbo.use()
        fbo.clear(*bg, 0.0, depth=1.0)
        mvp = (proj @ view).astype("f4")
        self.prog["mvp"].write(mvp.T.tobytes())
        self.prog["light"].value = tuple(float(x) for x in light)
        self.prog["eye"].value = tuple(float(x) for x in eye)
        self.prog["lit"].value = float(lit)
        held = []
        for m in meshes:
            tex = None
            ntex = None
            self.prog["use_nrm"].value = 0
            if "tex" in m:
                img = np.ascontiguousarray(m["tex"][::-1])  # GL row 0 is the bottom
                tex = self.ctx.texture((img.shape[1], img.shape[0]), 3, img.astype("u1").tobytes())
                tex.build_mipmaps()
                tex.use(0)
                self.prog["tex"].value = 0
                self.prog["use_tex"].value = 1
                uv = np.asarray(m["uv"], np.float32).copy()
                uv[:, 1] = 1 - uv[:, 1]
                if m.get("nmap") is not None and m.get("n") is not None:
                    nimg = np.ascontiguousarray(m["nmap"][::-1])
                    ntex = self.ctx.texture((nimg.shape[1], nimg.shape[0]), 3, nimg.astype("u1").tobytes())
                    ntex.build_mipmaps()
                    ntex.use(1)
                    self.prog["nmap"].value = 1
                    self.prog["use_nrm"].value = 1
                vao, buf = self._vao(self.prog, m["v"], m["f"], uv=uv, nrm=m.get("n"))
            else:
                self.prog["use_tex"].value = 0
                vao, buf = self._vao(self.prog, m["v"], m["f"], col=m.get("c"))
            vao.render(self.mgl.TRIANGLES)
            held += [vao, buf] + ([tex] if tex else []) + ([ntex] if ntex else [])
        raw = np.frombuffer(fbo.read(components=4), np.uint8).reshape(s, s, 4)[::-1]
        for h in held:
            h.release()
        img = raw.reshape(self.size, ssaa, self.size, ssaa, 4).mean(axis=(1, 3))
        return img[..., :3].round().astype(np.uint8), img[..., 3] / 255.0

    def mask(self, v, f, view, proj) -> np.ndarray:
        _, a = self.draw([{"v": v, "f": f}], view, proj, eye=(0, 0, 10), lit=0.0, ssaa=1)
        return a

    def positions(self, v, f, view, proj) -> tuple[np.ndarray, np.ndarray]:
        """The model-space position of the nearest surface under every pixel, and coverage.
        Row 0 is the top of the picture."""
        tri = np.asarray(v, np.float32)[np.asarray(f).reshape(-1)]
        buf = self.ctx.buffer(tri.astype("f4").tobytes())
        vao = self.ctx.vertex_array(self.pos_prog, [(buf, "3f", "in_pos")])
        self.pos_prog["mvp"].write((proj @ view).astype("f4").T.tobytes())
        fbo = self._fbo(self.size, self.size, "f4")
        fbo.use()
        fbo.clear(0.0, 0.0, 0.0, 0.0, depth=1.0)
        vao.render(self.mgl.TRIANGLES)
        raw = np.frombuffer(fbo.read(components=4, dtype="f4"), np.float32)
        raw = raw.reshape(self.size, self.size, 4)[::-1]
        vao.release()
        buf.release()
        return raw[..., :3].copy(), raw[..., 3] > 0.5

    def bake_positions(self, v, f, uv, size: int) -> tuple[np.ndarray, np.ndarray]:
        """Per texel: the 3D position and the face index (-1 where no face covers the texel).
        Row 0 is the top of the texture (glTF's v = 0)."""
        f = np.asarray(f)
        tri = np.asarray(v, np.float32)[f.reshape(-1)]
        t = np.asarray(uv, np.float32)[f.reshape(-1)].copy()
        t[:, 1] = 1 - t[:, 1]
        face = np.repeat(np.arange(len(f), dtype=np.float32), 3)[:, None]
        buf = self.ctx.buffer(np.hstack([tri, t, face]).astype("f4").tobytes())
        vao = self.ctx.vertex_array(self.bake_prog, [(buf, "3f 2f 1f", "in_pos", "in_uv", "in_face")])
        fbo = self._fbo(size, size, "f4")
        fbo.use()
        self.ctx.disable(self.mgl.DEPTH_TEST)
        fbo.clear(0.0, 0.0, 0.0, 0.0)
        vao.render(self.mgl.TRIANGLES)
        self.ctx.enable(self.mgl.DEPTH_TEST)
        raw = np.frombuffer(fbo.read(components=4, dtype="f4"), np.float32).reshape(size, size, 4)[::-1]
        vao.release()
        buf.release()
        return raw[..., :3].copy(), (raw[..., 3].round().astype(np.int64) - 1)
