# SPDX-License-Identifier: Apache-2.0
import unittest

import numpy as np

from shadowrunr_avatar3d import collar, geom, tex
from shadowrunr_avatar3d.glb import Mesh, Node, read_glb, write_glb


class SwatchTest(unittest.TestCase):
    def swatch(self):
        rng = np.random.default_rng(1)
        base = 0.7 + 0.1 * rng.standard_normal((90, 100, 1))
        return np.clip(base * np.array([1.0, 0.92, 0.8]), 0, 1)

    def test_detail_is_centred_on_one(self):
        d = tex.detail_map(self.swatch())
        self.assertAlmostEqual(float(d.mean()), 1.0, delta=0.03)
        self.assertGreater(float(d.std()), 0.03)

    def test_synth_keeps_contrast_and_is_deterministic(self):
        d = tex.detail_map(self.swatch())
        rng = np.random.default_rng(2)
        pos = rng.uniform(-0.5, 0.5, (4000, 3))
        n = pos / np.linalg.norm(pos, axis=1, keepdims=True)
        a = tex.synth_detail(d, pos, n, 800.0)
        b = tex.synth_detail(d, pos, n, 800.0)
        self.assertTrue(np.array_equal(a, b))
        self.assertAlmostEqual(float(a.mean()), 1.0, delta=0.05)
        # The mix must not wash out to flat: its spread stays near the swatch's own.
        self.assertGreater(float(a.std()), 0.6 * float(d.std()))

    def test_box_blur_is_a_mean(self):
        x = np.arange(36, dtype=float).reshape(6, 6)
        self.assertAlmostEqual(tex.box_blur(x, 1)[3, 3], x[2:5, 2:5].mean())


class NormalTest(unittest.TestCase):
    def quad(self):
        v = np.array([[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]], float)
        f = np.array([[0, 1, 2], [0, 2, 3]])
        # glTF UVs: v grows downward, so the model's +Y (up) is decreasing v.
        uv = np.array([[0, 1], [1, 1], [1, 0], [0, 0]], float)
        return v, f, uv

    def test_tangents_follow_u_and_image_up(self):
        v, f, uv = self.quad()
        t, b = tex.face_tangents(v, f, uv)
        np.testing.assert_allclose(t, [[1, 0, 0]] * 2, atol=1e-9)
        np.testing.assert_allclose(b, [[0, 1, 0]] * 2, atol=1e-9)

    def test_surface_normal_encodes_to_flat_blue(self):
        v, f, uv = self.quad()
        t, b = tex.face_tangents(v, f, uv)
        n = np.array([[0.0, 0.0, 1.0]] * 2)
        ts = tex.to_tangent_space(n, n, t, b)
        np.testing.assert_allclose(ts, [[0, 0, 1]] * 2, atol=1e-9)
        self.assertEqual(tex.encode_normals(ts)[0].tolist(), [128, 128, 255])

    def test_tilt_toward_u_and_up(self):
        v, f, uv = self.quad()
        t, b = tex.face_tangents(v, f, uv)
        n = np.array([[0.0, 0.0, 1.0]] * 2)
        tilt = np.array([[0.6, 0.0, 0.8], [0.0, 0.6, 0.8]])
        ts = tex.to_tangent_space(tilt, n, t, b)
        np.testing.assert_allclose(ts, tilt, atol=1e-9)

    def test_encode_round_trip(self):
        n = np.array([[0.0, 0.6, 0.8], [-0.6, 0.0, 0.8]])
        np.testing.assert_allclose(tex.decode_normals(tex.encode_normals(n)), n, atol=0.01)

    def test_barycentric(self):
        a, b, c = np.array([[0.0, 0, 0]]), np.array([[2.0, 0, 0]]), np.array([[0.0, 2, 0]])
        w = tex.barycentric(np.array([[0.5, 0.5, 0.0]]), a, b, c)
        np.testing.assert_allclose(w, [[0.5, 0.25, 0.25]])


class CavityTest(unittest.TestCase):
    def test_convex_sphere_reads_negative_and_a_dent_positive(self):
        rng = np.random.default_rng(3)
        d = rng.normal(size=(3000, 3))
        d /= np.linalg.norm(d, axis=1, keepdims=True)
        c = tex.cavity(d, d, k=8)
        self.assertLess(float(np.median(c)), 0.0)
        inward = tex.cavity(d, -d, k=8)  # the same points seen from the inside: concave
        self.assertGreater(float(np.median(inward)), 0.0)

    def test_factor_darkens_creases_more_than_it_lifts_edges(self):
        f = tex.cavity_factor(np.array([1.0, -1.0, 0.0]), np.array([1.0, -1.0, 0.0]), 0.5)
        self.assertLess(f[0], 1.0)
        self.assertGreater(f[1], 1.0)
        self.assertLess(1.0 - f[0], 0.51)
        self.assertGreater(1.0 - f[0], f[1] - 1.0)
        self.assertAlmostEqual(f[2], 1.0)


class CollarTest(unittest.TestCase):
    def test_split_bone_by_colour_and_place(self):
        v = np.array([[0, 0, 0], [1, 0, 0], [0, 1, 0], [5, 5, 5], [6, 5, 5], [5, 6, 5]], float)
        f = np.array([[0, 1, 2], [3, 4, 5]])
        c = np.array([[0.1, 0.1, 0.1]] * 3 + [[0.08, 0.07, 0.07]] * 3)  # both near black
        self.assertFalse(collar.split_bone(c, f).any())
        bone = collar.split_bone(c, f, v, np.array([[0.2, 0.2, 0.0]]), near=0.5)
        self.assertEqual(bone.tolist(), [True, False])
        pale = np.array([[0.7, 0.65, 0.58]] * 6)
        self.assertTrue(collar.split_bone(pale, f).all())
        gold = np.array([[0.8, 0.5, 0.1]] * 6)
        self.assertFalse(collar.split_bone(gold, f).any())

    def test_smooth_mask_removes_a_speck(self):
        # A strip of triangles, one flipped in the middle of a run.
        n = 12
        v = np.array([[i // 2, i % 2, 0] for i in range(n + 2)], float)
        f = np.array([[i, i + 1, i + 2] for i in range(n)])
        m = np.zeros(n, bool)
        m[4] = True
        self.assertFalse(collar.smooth_mask(f, m)[4])


class GlbMaterialTest(unittest.TestCase):
    def test_jpeg_atlas_and_normal_map_are_written(self):
        v = np.array([[0, 0, 0], [1, 0, 0], [0, 1, 0]], float)
        m = Mesh(v, np.array([[0, 1, 2]]), geom.vertex_normals(v, np.array([[0, 1, 2]])),
                 np.array([[0, 0], [1, 0], [0, 1]], np.float32), b"\xff\xd8base", texture_mime="image/jpeg",
                 normal_texture=b"\x89PNGnorm", normal_mime="image/png")
        g = read_glb(write_glb([Node("skull", mesh=m)]))
        mat = g.json["materials"][0]
        self.assertIn("normalTexture", mat)
        self.assertEqual([i["mimeType"] for i in g.json["images"]], ["image/jpeg", "image/png"])
        self.assertNotEqual(mat["normalTexture"]["index"], mat["pbrMetallicRoughness"]["baseColorTexture"]["index"])


if __name__ == "__main__":
    unittest.main()
