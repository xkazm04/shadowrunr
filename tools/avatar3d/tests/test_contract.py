# SPDX-License-Identifier: Apache-2.0
"""The GLB writer, reader and the lich contract check, on tiny meshes built here.

No model, no GPU and nothing from ``data/``: every mesh is a box drawn by the test."""

from __future__ import annotations

import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

import numpy as np

from shadowrunr_avatar3d import contract
from shadowrunr_avatar3d.__main__ import main
from shadowrunr_avatar3d.glb import Mesh, Node, read_glb, write_glb

# A 1x1 white PNG, so the texture path is exercised without Pillow.
PNG = bytes.fromhex(
    "89504e470d0a1a0a0000000d4948445200000001000000010806000000"
    "1f15c4890000000d49444154789c63f8ffff3f0005fe02fea7d6a4c20000000049454e44ae426082"
)


def box(lo, hi) -> tuple[np.ndarray, np.ndarray]:
    lo, hi = np.asarray(lo, float), np.asarray(hi, float)
    v = np.array([[x, y, z] for x in (0, 1) for y in (0, 1) for z in (0, 1)], float)
    v = lo + v * (hi - lo)
    f = np.array(
        [[0, 1, 3], [0, 3, 2], [4, 6, 7], [4, 7, 5], [0, 4, 5], [0, 5, 1],
         [2, 3, 7], [2, 7, 6], [0, 2, 6], [0, 6, 4], [1, 5, 7], [1, 7, 3]]
    )
    return v, f


def lich_nodes(jaw_pivot=(0.0, -0.2, -0.05), jaw_box=((-0.3, -0.3, 0.0), (0.3, 0.0, 0.4)),
               eyes=((-0.2, 0.1, 0.3), (0.2, 0.1, 0.3)), names=contract.NODE_NAMES,
               collar: bool = False) -> list[Node]:
    sv, sf = box((-0.4, -0.2, -0.5), (0.4, 0.5, 0.5))
    jv, jf = box(*jaw_box)
    uv = np.zeros((8, 2), np.float32)
    meshes = {
        "skull": Mesh(sv, sf, uvs=uv, texture_png=PNG),
        "jaw": Mesh(jv, jf, uvs=uv, texture_png=PNG),
    }
    where = {
        "skull": (0.0, 0.0, 0.0),
        "jaw": jaw_pivot,
        "eye_l": eyes[0],
        "eye_r": eyes[1],
        "monocle": (eyes[1][0], eyes[1][1], eyes[1][2] + 0.05),
        "neck": (0.0, -0.3, -0.1),
    }
    out = [Node(n, where.get(n, (0.0, 0.0, 0.0)), meshes.get(n)) for n in names]
    if collar:
        cv, cf = box((-0.6, -0.4, -0.3), (0.6, 0.0, 0.3))
        out.append(Node("collar", (0.0, -0.3, -0.1), Mesh(cv, cf, uvs=uv, texture_png=PNG)))
    return out


def meta(**over) -> dict:
    m = {
        "version": 1,
        "source_model": "synthetic",
        "portrait_sha256": "0" * 64,
        "tris": 24,
        "jaw_open_rad": 0.35,
        "eye_radius": 0.06,
        "camera": {"fov": 30.0, "pos": [0.0, 0.1, 2.5], "target": [0.0, 0.1, 0.0]},
    }
    m.update(over)
    return m


class GlbRoundTrip(unittest.TestCase):
    def test_nodes_meshes_and_texture_survive(self):
        data = write_glb(lich_nodes())
        self.assertEqual(len(data) % 4, 0)
        g = read_glb(data)
        self.assertEqual([n["name"] for n in g.json["nodes"]], list(contract.NODE_NAMES))
        by = g.nodes_by_name()
        v, f = g.mesh_arrays(by["jaw"]["mesh"])
        self.assertEqual(v.shape, (8, 3))
        self.assertEqual(f.shape, (12, 3))
        self.assertAlmostEqual(by["jaw"]["translation"][1], -0.2, places=6)
        self.assertNotIn("translation", by["skull"])
        img = g.json["images"][0]
        view = g.json["bufferViews"][img["bufferView"]]
        raw = g.bin[view["byteOffset"] : view["byteOffset"] + view["byteLength"]]
        self.assertEqual(raw, PNG)
        self.assertEqual(len(g.json["images"]), 1)  # skull and jaw share the atlas

    def test_rejects_garbage(self):
        rep = contract.Report()
        contract.check_glb(b"not a glb at all, really", rep)
        self.assertFalse(rep.ok)


class ContractCheck(unittest.TestCase):
    def run_check(self, nodes, m=None, **kw) -> contract.Report:
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "lich.glb"
            p.write_bytes(write_glb(nodes))
            (Path(d) / "lich.json").write_text(json.dumps(m or meta()), encoding="utf-8")
            return contract.check(p, **kw)

    def test_good_asset_passes(self):
        rep = self.run_check(lich_nodes())
        self.assertTrue(rep.ok, rep.errors)
        self.assertEqual(rep.facts["tris"], 24)

    def test_missing_and_extra_nodes(self):
        names = [n for n in contract.NODE_NAMES if n != "monocle"] + ["crown"]
        rep = self.run_check(lich_nodes(names=names))
        self.assertTrue(any("'monocle'" in e for e in rep.errors), rep.errors)
        self.assertTrue(any("crown" in e for e in rep.errors), rep.errors)

    def test_optional_collar_passes_and_counts(self):
        rep = self.run_check(lich_nodes(collar=True), m=meta(tris=36))
        self.assertTrue(rep.ok, rep.errors)
        self.assertEqual(rep.facts["tris"], 36)
        self.assertEqual(rep.facts["collar_tris"], 12)

    def test_collar_must_be_a_mesh(self):
        nodes = lich_nodes() + [Node("collar", (0.0, -0.3, -0.1), None)]
        rep = self.run_check(nodes)
        self.assertTrue(any("'collar' has no mesh" in e for e in rep.errors), rep.errors)

    def test_jaw_pivot_at_the_chin_is_caught(self):
        # Pivot at the bottom front: the jaw mesh rises above and behind its hinge.
        rep = self.run_check(
            lich_nodes(jaw_pivot=(0.0, -0.5, 0.35), jaw_box=((-0.3, 0.0, -0.4), (0.3, 0.3, 0.0)))
        )
        self.assertTrue(any("top of the jaw" in e for e in rep.errors), rep.errors)
        self.assertTrue(any("back of the jaw" in e for e in rep.errors), rep.errors)

    def test_off_centre_pivot_is_caught(self):
        rep = self.run_check(lich_nodes(jaw_box=((0.0, -0.3, 0.0), (0.6, 0.0, 0.4))))
        self.assertTrue(any("centred" in e for e in rep.errors), rep.errors)

    def test_swapped_eyes_are_caught(self):
        rep = self.run_check(lich_nodes(eyes=((0.2, 0.1, 0.3), (-0.2, 0.1, 0.3))))
        self.assertTrue(any("eye_l must have negative X" in e for e in rep.errors), rep.errors)

    def test_size_and_triangle_budgets(self):
        rep = self.run_check(lich_nodes(), max_bytes=100)
        self.assertTrue(any("byte budget" in e for e in rep.errors), rep.errors)
        rep = self.run_check(lich_nodes(), max_tris=10)
        self.assertTrue(any("triangle" in e for e in rep.errors), rep.errors)

    def test_meta_must_match_and_validate(self):
        rep = self.run_check(lich_nodes(), meta(tris=99))
        self.assertTrue(any("tris 99" in e for e in rep.errors), rep.errors)
        rep = self.run_check(lich_nodes(), meta(portrait_sha256="xyz", eye_radius=None))
        self.assertTrue(any("portrait_sha256" in e for e in rep.errors), rep.errors)
        self.assertTrue(any("null" in e for e in rep.errors), rep.errors)
        rep = self.run_check(lich_nodes(), meta(camera={"fov": 30, "pos": [0, 0, 1]}))
        self.assertTrue(any("camera.target" in e for e in rep.errors), rep.errors)

    def test_height_must_be_one_unit(self):
        nodes = lich_nodes()
        nodes[0].mesh.positions = nodes[0].mesh.positions * 2
        rep = self.run_check(nodes)
        self.assertTrue(any("1 unit" in e for e in rep.errors), rep.errors)


class CheckCommand(unittest.TestCase):
    def test_exit_codes(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "lich.glb"
            p.write_bytes(write_glb(lich_nodes()))
            (Path(d) / "lich.json").write_text(json.dumps(meta()), encoding="utf-8")
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(main(["--check", str(p)]), 0)
                (Path(d) / "lich.json").write_text(json.dumps(meta(version=2)), encoding="utf-8")
                self.assertEqual(main(["--check", str(p)]), 1)
            self.assertIn("version 2, expected 1", out.getvalue())


if __name__ == "__main__":
    unittest.main()
