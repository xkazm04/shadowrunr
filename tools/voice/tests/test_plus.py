# SPDX-License-Identifier: Apache-2.0
"""Kokoro Plus (``say.delivery``): the cue, the tables, the plan, the mouth shapes over the
inserted silence, the recipe's ``[emotions]``, and the sidecar over the fake engines. No engine,
no models; the pitch through the real chain runs only where numpy is importable."""

from __future__ import annotations

import base64
import importlib.util
import json
import os
import tempfile
import tomllib
import unittest
from array import array
from pathlib import Path
from typing import Any

from shadowrunr_voice.blend import (
    ROW_FLOATS,
    BlendError,
    assign_slots,
    build,
    load_emotions,
    load_recipes,
    sid_of,
)
from shadowrunr_voice.engine import SPEAKER_BYTES, Rendered, build_args, paths_for
from shadowrunr_voice.plus import (
    EMOTIONS,
    LEVERS,
    PITCH_FLOOR_ST,
    Delivery,
    EmotionVoice,
    clamp_note,
    clauses,
    cut,
    edges,
    emotion_voices,
    insert_silence,
    parse_delivery,
    plan_line,
    trim_spans,
)
from shadowrunr_voice.sidecar import Faces, Settings, Sidecar, hello
from shadowrunr_voice.text import split_line
from shadowrunr_voice.visemes import to_ms
from shadowrunr_voice.warm import request_of

from .test_blend import RECIPE, speaker, voices_bytes
from .test_engine import FAKE, engine_dir
from .test_protocol import FakeDurations, Sink
from .test_warm import FAKE_WARM

HAVE_NUMPY = importlib.util.find_spec("numpy") is not None
SR = 24_000
LIGHT_ST = -2.5
COMMITTED = Path(__file__).resolve().parent.parent / "recipes" / "malakor.toml"
#: The voice lab's blends: the committed recipe's sids and register corrections.
LAB = {
    "menace": EmotionVoice(50, 0.21),
    "glee": EmotionVoice(49, -1.70),
    "deadpan": EmotionVoice(48, -1.99),
    "hushed": EmotionVoice(47, -2.67),
    "outrage": EmotionVoice(46, -3.21),
    "sarcasm": EmotionVoice(45, -2.22),
    "weary": EmotionVoice(44, -1.29),
    "triumph": EmotionVoice(43, -3.21),
    "tender": EmotionVoice(42, -1.22),
}


def plan(text: str, emotion: str, intensity: float, pace: str, **kw: Any) -> Any:
    kw.setdefault("base_sid", 52)
    kw.setdefault("voices", LAB)
    kw.setdefault("preset_st", LIGHT_ST)
    return plan_line(text, Delivery(emotion, intensity, pace), **kw)


class Cue(unittest.TestCase):
    def test_a_delivery_parses_and_its_intensity_is_clamped(self) -> None:
        self.assertEqual(
            parse_delivery({"emotion": "menace", "intensity": 0.8, "pace": "slow"}),
            Delivery("menace", 0.8, "slow"),
        )
        self.assertEqual(parse_delivery({"emotion": "glee", "intensity": 3}).intensity, 1.0)
        self.assertEqual(parse_delivery({"emotion": "glee", "intensity": -1}).intensity, 0.0)
        for odd in (None, "loud", True, float("nan")):
            self.assertEqual(parse_delivery({"emotion": "glee", "intensity": odd}).intensity, 0.6)
        self.assertEqual(parse_delivery({"emotion": "glee"}), Delivery("glee", 0.6, "normal"))
        self.assertEqual(parse_delivery({"pace": "brisk"}).pace, "normal")

    def test_the_face_keys_are_accepted_and_ignored(self) -> None:
        # The cue may direct the face (gesture, eye_light); the voice neither reads nor speaks them.
        face = {"emotion": "menace", "intensity": 0.9, "pace": "slow"}
        face.update(gesture="glare", eye_light=2.2)
        self.assertEqual(parse_delivery(face), Delivery("menace", 0.9, "slow"))
        alone = {"emotion": "neutral", "intensity": 0.6, "pace": "normal"}
        alone.update(gesture="nod", eye_light=0.4)
        self.assertEqual(parse_delivery(alone), Delivery("neutral", 0.6, "normal"))

    def test_no_delivery_is_none(self) -> None:
        for raw in (None, "menace", 5, [], ["menace"]):
            self.assertIsNone(parse_delivery(raw), raw)

    def test_an_unknown_emotion_is_neutral(self) -> None:
        odd = parse_delivery({"emotion": "joy", "intensity": 0.9, "pace": "fast"})
        self.assertEqual(odd, Delivery("neutral", 0.9, "fast"))
        self.assertEqual(parse_delivery({"emotion": 7}).emotion, "neutral")
        text = "Eleven centuries. One more."
        self.assertEqual(
            plan_line(text, odd, base_sid=52, voices=LAB, preset_st=LIGHT_ST),
            plan(text, "neutral", 0.9, "fast"),
        )

    def test_the_table_covers_the_cue_set(self) -> None:
        self.assertEqual(set(LEVERS), set(EMOTIONS))


class Tables(unittest.TestCase):
    def test_speed_is_the_pace_nudged_by_the_emotion(self) -> None:
        line = "Kneel."
        self.assertEqual(plan(line, "neutral", 0.6, "slow").speed, 0.85)
        self.assertEqual(plan(line, "neutral", 0.6, "normal").speed, 1.0)
        self.assertEqual(plan(line, "neutral", 0.6, "fast").speed, 1.18)
        self.assertEqual(plan(line, "menace", 0.8, "slow").speed, 0.8)
        self.assertEqual(plan(line, "glee", 0.9, "fast").speed, 1.23)
        self.assertEqual(plan(line, "outrage", 1.0, "fast").speed, 1.08)
        # The nudge does not scale: a faint menace is still a slower one.
        self.assertEqual(plan(line, "menace", 0.1, "normal").speed, 0.95)

    def test_sentence_pauses_scale_with_the_intensity_and_the_last_part_has_none(self) -> None:
        text = "Minion. Come closer. No... closer. I want you to hear."
        p = plan(text, "menace", 0.8, "normal")
        self.assertEqual(
            p.parts, ["Minion.", "Come closer.", "No... closer.", "I want you to hear."]
        )
        self.assertEqual(p.pauses_ms, [448, 448, 448, 0])
        self.assertEqual(plan(text, "menace", 0.4, "normal").pauses_ms, [224, 224, 224, 0])
        self.assertEqual(plan(text, "menace", 0.0, "normal").pauses_ms, [0, 0, 0, 0])

    def test_slow_pace_also_pauses_at_clauses(self) -> None:
        text = "Bone beetle, I'm sorry about your dog. Pull up a chair by the fire."
        slow = plan(text, "tender", 0.6, "slow")
        self.assertEqual(
            slow.parts,
            ["Bone beetle,", "I'm sorry about your dog.", "Pull up a chair by the fire."],
        )
        self.assertEqual(slow.pauses_ms, [399, 324, 0])
        normal = plan(text, "tender", 0.6, "normal")
        self.assertEqual(
            normal.parts, ["Bone beetle, I'm sorry about your dog.", "Pull up a chair by the fire."]
        )
        self.assertEqual(
            clauses("One, two; three: four, 1,000."), ["One,", "two;", "three:", "four,", "1,000."]
        )

    def test_short_sentences_are_beats_of_their_own(self) -> None:
        text = "I explain it again. Every. Single. Stream."
        self.assertEqual(len(split_line(text)), 1)
        p = plan(text, "weary", 0.7, "normal")
        self.assertEqual(p.parts, ["I explain it again.", "Every.", "Single.", "Stream."])
        self.assertEqual(p.pauses_ms, [301, 301, 301, 0])

    def test_a_long_first_sentence_is_cut_for_latency_without_a_pause(self) -> None:
        first = "Research out, cards up, strings pinned, chat roasted, coffee cold, " * 2
        p = plan(first.strip().rstrip(",") + ". Silence.", "glee", 0.8, "fast")
        self.assertEqual(len(p.parts), 3)
        self.assertEqual(p.pauses_ms, [0, 116, 0])

    def test_the_bench_lines_get_the_lab_s_pauses_back(self) -> None:
        # (emotion, the lab's intensity, the lab's mean sentence pause in ms)
        for emotion, intensity, lab_ms in (
            ("menace", 0.8, 450), ("glee", 0.9, 130), ("deadpan", 0.6, 250),
            ("outrage", 1.0, 207), ("sarcasm", 0.8, 350), ("weary", 0.7, 300),
            ("triumph", 1.0, 275), ("tender", 0.6, 325),
        ):  # fmt: skip
            got = LEVERS[emotion].sentence_ms * intensity
            self.assertLess(abs(got - lab_ms), 5, emotion)


class EmotionVoices(unittest.TestCase):
    def test_the_blend_speaks_from_the_floor_up_and_the_base_below_it(self) -> None:
        self.assertEqual(plan("Kneel.", "menace", 0.4, "normal").sid, 50)
        self.assertEqual(plan("Kneel.", "menace", 0.39, "normal").sid, 52)
        self.assertEqual(plan("Kneel.", "neutral", 1.0, "normal").sid, 52)
        self.assertEqual(plan("Kneel.", "menace", 1.0, "normal", voices={}).sid, 52)

    def test_the_voices_json_gives_the_base_s_blends_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            voices = Path(tmp) / "v.bin"
            voices.write_bytes(b"")
            meta = {
                "emotions": [
                    {"name": "menace", "sid": 50, "base_sid": 52, "pitch_correction": 0.21},
                    {"name": "glee", "sid": 49, "base_sid": 51, "pitch_correction": -1.7},
                    {"name": "neutral", "sid": 48, "base_sid": 52},
                    {"name": "weary", "sid": 99, "base_sid": 52},
                    {"name": "joy", "sid": 47, "base_sid": 52},
                ]
            }
            Path(tmp, "v.bin.json").write_text(json.dumps(meta), encoding="utf-8")
            self.assertEqual(emotion_voices(voices, 52), {"menace": EmotionVoice(50, 0.21)})
            self.assertEqual(emotion_voices(voices, 51), {"glee": EmotionVoice(49, -1.7)})
            Path(tmp, "v.bin.json").write_text("not json", encoding="utf-8")
            self.assertEqual(emotion_voices(voices, 52), {})
            self.assertEqual(emotion_voices(Path(tmp) / "none.bin", 52), {})


class Pitch(unittest.TestCase):
    def test_the_emotion_s_semitones_and_the_blend_s_correction_add(self) -> None:
        p = plan("Kneel.", "menace", 0.8, "normal")
        self.assertAlmostEqual(p.pitch_st, -1.25 * 0.8 + 0.21)
        self.assertFalse(p.clamped)
        base = plan("Kneel.", "menace", 0.3, "normal")  # the base voice: no correction
        self.assertAlmostEqual(base.pitch_st, -1.25 * 0.3)

    def test_a_total_past_the_floor_is_held_there(self) -> None:
        p = plan("Kneel.", "outrage", 1.0, "fast")  # -2.5 + 1.0 - 3.21 = -4.71
        self.assertTrue(p.clamped)
        self.assertAlmostEqual(LIGHT_ST + p.pitch_st, PITCH_FLOOR_ST)
        self.assertAlmostEqual(p.pitch_st, -1.5)

    def test_without_a_chain_there_is_no_pitch_lever(self) -> None:
        p = plan("Kneel.", "outrage", 1.0, "fast", preset_st=None)
        self.assertEqual((p.pitch_st, p.clamped), (0.0, False))
        self.assertEqual(clamp_note(LAB, None), [])

    def test_the_held_emotions_are_named(self) -> None:
        held = clamp_note(LAB, LIGHT_ST)
        self.assertIn("outrage", held)
        self.assertIn("hushed", held)
        self.assertNotIn("menace", held)
        self.assertNotIn("glee", held)
        self.assertEqual(clamp_note({}, LIGHT_ST), [])


class Silence(unittest.TestCase):
    def test_only_the_quiet_at_a_pause_s_edge_is_cut_to_twenty_ms(self) -> None:
        quiet, loud = [0] * 2400, [9000, -9000] * 1200  # 100 ms of each
        pcm = array("h", quiet + loud + quiet).tobytes()
        self.assertEqual(edges(pcm, SR, lead=False, tail=False), (0, 7200))
        self.assertEqual(edges(pcm, SR, lead=True, tail=True), (2400 - 480, 4800 + 480))
        self.assertEqual(edges(pcm, SR, lead=False, tail=True), (0, 4800 + 480))
        self.assertEqual(edges(bytes(4800), SR, lead=True, tail=True), (0, 2400))
        kept = array("h")
        kept.frombytes(cut(pcm, 1920, 5280, SR))
        self.assertEqual(len(kept), 3360)
        self.assertEqual(kept[480:2880].tolist(), loud)  # the speech itself is untouched

    def test_the_kept_spans_start_at_zero(self) -> None:
        spans = [["sil", 0, 2400], ["aa", 2400, 4800], ["sil", 4800, 7200]]
        self.assertEqual(
            trim_spans(spans, 1920, 5280), [["sil", 0, 480], ["aa", 480, 2880], ["sil", 2880, 3360]]
        )

    def test_spans_after_the_gap_move_by_it_and_one_across_it_is_split(self) -> None:
        spans = [["aa", 0, 100], ["sil", 100, 150], ["E", 150, 300]]
        self.assertEqual(
            insert_silence(spans, 200, 50),
            [
                ["aa", 0, 100],
                ["sil", 100, 150],
                ["E", 150, 200],
                ["sil", 200, 250],
                ["E", 250, 350],
            ],
        )
        self.assertEqual(
            insert_silence(spans, 100, 50),
            [["aa", 0, 100], ["sil", 100, 200], ["E", 200, 350]],
        )

    def test_silence_at_the_end_ends_the_timeline_at_the_audio_end(self) -> None:
        spans = [["aa", 0, 2400], ["E", 2400, 4800]]
        out = insert_silence(spans, 4800, 7200)
        self.assertEqual(out[-1], ["sil", 4800, 12_000])
        ms = to_ms(out, 12_000, SR)
        self.assertIsNotNone(ms)
        self.assertEqual(ms[-1], ["sil", 200, 500])  # type: ignore[index]
        self.assertEqual(insert_silence(spans, 4800, 0), spans)

    def test_the_face_pads_its_shapes_with_the_pause(self) -> None:
        durations = FakeDurations()
        faces = Faces(durations, {16: "ə", 50: " "}, 52)  # type: ignore[arg-type]
        words = 3
        pcm = b"".join(
            (8000 if i % 40 < 20 else -8000).to_bytes(2, "little", signed=True)
            for i in range(2400 * words)
        )
        rendered = Rendered(
            pcm=pcm,
            tokens=[[0, 50, *([16] * words), 0]],
            raw_samples=2400 * words,
            time_map=[(0, 2400 * words, 0, 2400 * words)],
        )
        vis = faces.visemes(rendered, SR, sid=50, speed=0.8, pad=4800)
        self.assertIsNotNone(vis)
        self.assertEqual(vis[-1], ["sil", 300, 500])  # type: ignore[index]
        self.assertEqual(durations.calls, [(50, 0.8)])
        kept = faces.visemes(rendered, SR, sid=50, speed=0.8, keep=(1200, 6000), pad=2400)
        self.assertIsNotNone(kept)
        self.assertEqual(kept[0][1], 0)  # type: ignore[index]
        self.assertEqual(kept[-1], ["sil", 200, 300])  # type: ignore[index]


class Recipe(unittest.TestCase):
    def test_the_committed_recipe_carries_the_lab_s_emotion_blends(self) -> None:
        table = tomllib.loads(COMMITTED.read_text(encoding="utf-8"))
        voices = assign_slots(load_recipes(table))
        emotions = load_emotions(table, voices)
        self.assertEqual({e.name: EmotionVoice(e.sid, e.pitch_correction) for e in emotions}, LAB)
        for e in emotions:
            self.assertEqual(e.base, "malakor-a")
            self.assertGreaterEqual(dict(e.mix)["malakor-a"], 0.6)
            self.assertAlmostEqual(sum(w for _, w in e.mix), 1.0)

    def test_bad_emotion_blends_are_refused(self) -> None:
        voices = assign_slots(load_recipes(tomllib.loads(RECIPE)))
        for bad in (
            '[emotions.joy]\n"malakor-a" = 0.7\nam_puck = 0.3\n',
            '[emotions.neutral]\n"malakor-a" = 0.7\nam_puck = 0.3\n',
            '[emotions.glee]\n"malakor-a" = 0.5\nam_puck = 0.5\n',
            "[emotions.glee]\nam_onyx = 0.7\nam_puck = 0.3\n",
            '[emotions.glee]\n"malakor-a" = 0.4\n"malakor-b" = 0.4\nam_puck = 0.2\n',
            '[emotions.glee]\n"malakor-a" = 0.7\nam_nobody = 0.3\n',
            '[emotions.glee]\n"malakor-a" = 0.7\nam_puck = 0.3\npitch_correction = "low"\n',
            '[emotions.glee]\n"malakor-a" = 0.7\nam_puck = 0.3\nsid = "x"\n',
        ):
            with self.assertRaises(BlendError, msg=bad):
                load_emotions(tomllib.loads(RECIPE + bad), voices)
        self.assertEqual(load_emotions(tomllib.loads(RECIPE), voices), [])

    def test_build_mixes_each_emotion_from_its_base_row_and_lists_it(self) -> None:
        recipe = RECIPE + (
            '\n[emotions.menace]\n"malakor-a" = 0.65\nam_onyx = 0.35\nsid = 50\n'
            "pitch_correction = 0.21\n"
            '\n[emotions.glee]\n"malakor-a" = 0.6\nam_puck = 0.4\npitch_correction = -1.7\n'
        )
        with tempfile.TemporaryDirectory() as tmp:
            stock = Path(tmp) / "voices.bin"
            stock.write_bytes(voices_bytes())
            src = Path(tmp) / "malakor.toml"
            src.write_text(recipe, encoding="utf-8")
            plain = Path(tmp) / "plain.toml"
            plain.write_text(RECIPE, encoding="utf-8")
            out = Path(tmp) / "out.bin"
            meta = build(stock, src, out)
            build(stock, plain, Path(tmp) / "plain.bin")
            raw, before = out.read_bytes(), (Path(tmp) / "plain.bin").read_bytes()
            self.assertEqual(len(raw), len(before))
            # The base slots are the same bytes as without the emotions.
            for sid in (52, 51):
                self.assertEqual(speaker(raw, sid), speaker(before, sid))
            base, onyx = speaker(raw, 52), speaker(raw, sid_of("am_onyx"))
            want = array("f", [0.65 * a + 0.35 * o for a, o in zip(base, onyx)])
            self.assertEqual(speaker(raw, 50), want)
            listed = {e["name"]: e for e in meta["emotions"]}
            self.assertEqual((listed["menace"]["sid"], listed["menace"]["base_sid"]), (50, 52))
            self.assertEqual(listed["glee"]["sid"], 49)  # the highest free slot
            changed = [
                s
                for s in range(len(raw) // (ROW_FLOATS * 4))
                if speaker(raw, s) != speaker(before, s)
            ]
            self.assertEqual(changed, [49, 50])
            self.assertEqual(
                emotion_voices(out, 52),
                {"menace": EmotionVoice(50, 0.21), "glee": EmotionVoice(49, -1.7)},
            )


class Wire(unittest.TestCase):
    def test_a_warm_request_s_sid_and_speed_are_read_or_ignored(self) -> None:
        self.assertEqual(request_of({"text": "Hi.", "sid": 50, "speed": 0.8}), ("Hi.", 50, 0.8))
        self.assertEqual(request_of({"text": "Hi."}), ("Hi.", None, None))
        self.assertEqual(request_of({"text": "Hi.", "sid": True, "speed": 9}), ("Hi.", None, None))
        self.assertEqual(request_of({"text": 3, "sid": -1, "speed": "x"}), ("", None, None))

    def test_the_engine_s_speed_flag_is_there_only_when_asked(self) -> None:
        paths = paths_for(Path("E"))
        self.assertFalse(
            any(a.startswith("--speed") for a in build_args(paths, 3, Path("o"), "Hi."))
        )
        args = build_args(paths, 50, Path("o"), "Hi.", speed=0.8)
        self.assertIn("--speed=0.8", args)
        self.assertLess(args.index("--sid=50"), args.index("--speed=0.8"))


class Speaking(unittest.TestCase):
    """The sidecar over the fake binary, with a two-speaker voices file whose ``.json`` gives
    sid 1's menace blend in slot 0."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.root = engine_dir(base / "engine")
        (self.root / "kokoro" / "voices.bin").write_bytes(bytes(2 * SPEAKER_BYTES))
        self.voices = base / "voices" / "lich-voices.bin"
        self.voices.parent.mkdir()
        self.voices.write_bytes(bytes(2 * SPEAKER_BYTES))
        meta = {"emotions": [{"name": "menace", "sid": 0, "base_sid": 1, "pitch_correction": 0.21}]}
        Path(str(self.voices) + ".json").write_text(json.dumps(meta), encoding="utf-8")
        self.args = base / "args.txt"
        self.requests = base / "requests.txt"
        os.environ["FAKE_ENGINE_ARGS"] = str(self.args)
        os.environ["FAKE_WARM_REQUESTS"] = str(self.requests)
        self.sink = Sink()
        self.sidecars: list[Sidecar] = []

    def tearDown(self) -> None:
        for sidecar in self.sidecars:
            sidecar.close()
        del os.environ["FAKE_ENGINE_ARGS"]
        del os.environ["FAKE_WARM_REQUESTS"]
        self.tmp.cleanup()

    def settings(self, **kw: Any) -> Settings:
        kw.setdefault("command", FAKE)
        kw.setdefault("voices", self.voices)
        return Settings(engine_dir=self.root, sid=1, **kw)

    def sidecar(self, settings: Settings, **kw: Any) -> Sidecar:
        sidecar = Sidecar(settings, self.sink.send, **kw)
        self.sidecars.append(sidecar)
        return sidecar

    def say(self, sidecar: Sidecar, ident: str, text: str, **extra: Any) -> list[dict[str, Any]]:
        before = len(self.sink.messages)
        sidecar.feed(json.dumps({"type": "say", "id": ident, "text": text, **extra}))
        got = self.sink.wait_for(lambda m: any(x.get("last") for x in m[before:]))
        return [m for m in got[before:] if m["id"] == ident]

    def test_the_hello_says_plus_and_names_the_held_pitch_once(self) -> None:
        message, dead, _, _ = hello(self.settings())
        self.assertIsNone(dead)
        self.assertIs(message["plus"], True)
        self.assertIn("plus (1 emotion voice", message["reason"])
        if HAVE_NUMPY:
            meta = {
                "emotions": [
                    {"name": "outrage", "sid": 0, "base_sid": 1, "pitch_correction": -3.21}
                ]
            }
            Path(str(self.voices) + ".json").write_text(json.dumps(meta), encoding="utf-8")
            light, _, _, _ = hello(self.settings(fx="light"))
            self.assertEqual(light["reason"].count("pitch held at -4.0 st for outrage"), 1)
        dead_message, dead, _, _ = hello(self.settings(voices=self.voices.with_name("gone.bin")))
        self.assertIsNotNone(dead)
        self.assertNotIn("plus", dead_message)

    def test_no_delivery_is_today_s_output(self) -> None:
        faces = Faces(FakeDurations(), {16: "ə", 50: " "}, 1)  # type: ignore[arg-type]
        dry_seen: list[bytes] = []
        pitched_seen: list[float] = []

        def wet(pcm: bytes) -> bytes:
            dry_seen.append(pcm)
            return pcm[::-1]

        def pitched(pcm: bytes, semitones: float) -> bytes:
            pitched_seen.append(semitones)
            return pcm

        sidecar = self.sidecar(self.settings(fx="light"), faces=faces, wet=wet, pitched=pitched)
        text = "Kneel. Kneel before your lich, minion."
        got = self.say(sidecar, "a", text)
        null = self.say(sidecar, "b", text, delivery=None)
        self.assertEqual([m["text"] for m in got], split_line(text))
        self.assertEqual(
            [(m["pcm"], m["visemes"]) for m in got], [(m["pcm"], m["visemes"]) for m in null]
        )
        for message, dry in zip(got, dry_seen):
            self.assertEqual(base64.b64decode(message["pcm"]), dry[::-1])
            self.assertEqual(len(dry), 2400 * len(message["text"].split()) * 2)  # no pause
        self.assertEqual(pitched_seen, [])
        self.assertEqual(faces.durations.calls, [(1, 1.0)] * 4)  # type: ignore[attr-defined]
        asked = self.args.read_text(encoding="utf-8").splitlines()
        self.assertTrue(all(line.startswith("1 - ") for line in asked), asked)

    def test_a_delivery_renders_with_its_voice_speed_and_pauses(self) -> None:
        faces = Faces(FakeDurations(), {16: "ə", 50: " "}, 1)  # type: ignore[arg-type]
        sidecar = self.sidecar(self.settings(), faces=faces)
        delivery = {"emotion": "menace", "intensity": 0.8, "pace": "slow"}
        got = self.say(sidecar, "m", "Minion, come closer. No closer.", delivery=delivery)
        self.assertEqual([m["text"] for m in got], ["Minion,", "come closer.", "No closer."])
        pauses = [300, 448, 0]  # clause 375 x 0.8, sentence 560 x 0.8
        for message, pause in zip(got, pauses):
            pcm = base64.b64decode(message["pcm"])
            words = len(message["text"].split())
            pad = round(pause * SR / 1000)
            self.assertEqual(len(pcm) // 2, 2400 * words + pad)
            self.assertEqual(pcm[len(pcm) - 2 * pad :], bytes(2 * pad))
            vis = message["visemes"]
            self.assertEqual(vis[-1][2], round((2400 * words + pad) * 1000 / SR))
            if pad:
                self.assertEqual(vis[-1][0], "sil")
        self.assertEqual(set(faces.durations.calls), {(0, 0.8)})  # type: ignore[attr-defined]
        asked = self.args.read_text(encoding="utf-8").splitlines()
        self.assertEqual([line.split(" ", 2)[:2] for line in asked], [["0", "0.8"]] * 3)

    def test_a_delivery_s_face_keys_change_nothing_the_voice_does(self) -> None:
        faces = Faces(FakeDurations(), {16: "ə", 50: " "}, 1)  # type: ignore[arg-type]
        sidecar = self.sidecar(self.settings(), faces=faces)
        text = "Kneel. Now."
        plain = {"emotion": "menace", "intensity": 0.9, "pace": "slow"}
        face = {**plain, "gesture": "glare", "eye_light": 2.2}
        a = self.say(sidecar, "p", text, delivery=plain)
        b = self.say(sidecar, "g", text, delivery=face)
        self.assertEqual(
            [(m["text"], m["pcm"], m["visemes"]) for m in a],
            [(m["text"], m["pcm"], m["visemes"]) for m in b],
        )
        self.assertNotIn("glare", "".join(m["text"] for m in b))
        asked = self.args.read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(asked), 4)
        self.assertEqual(sorted(asked[:2]), sorted(asked[2:]))  # parts render in parallel
        self.assertFalse(any("glare" in line or "2.2" in line for line in asked), asked)

    def test_a_faint_or_unknown_emotion_keeps_the_base_voice(self) -> None:
        sidecar = self.sidecar(self.settings())
        self.say(sidecar, "f", "Hm.", delivery={"emotion": "menace", "intensity": 0.2})
        self.say(sidecar, "u", "Hm.", delivery={"emotion": "joy", "intensity": 0.9, "pace": "fast"})
        asked = [
            line.split(" ", 2)[:2] for line in self.args.read_text(encoding="utf-8").splitlines()
        ]
        self.assertEqual(asked, [["1", "0.95"], ["1", "1.18"]])

    def test_the_warm_worker_gets_a_sid_and_speed_only_with_a_delivery(self) -> None:
        sidecar_settings = self.settings(
            command=None, engine="warm", warm_command=FAKE_WARM, workers=1
        )
        message, dead, _, warm = hello(sidecar_settings)
        self.assertIsNone(dead, message)
        sidecar = self.sidecar(sidecar_settings, warm=warm)
        self.say(sidecar, "a", "Plain words.")
        self.say(sidecar, "b", "Dark words.", delivery={"emotion": "menace", "intensity": 0.9})
        sent = [json.loads(line) for line in self.requests.read_text(encoding="utf-8").splitlines()]
        self.assertEqual(sorted(sent[0]), ["text", "urgent"])
        self.assertEqual((sent[1]["sid"], sent[1]["speed"]), (0, 0.95))

    @unittest.skipUnless(HAVE_NUMPY, "the lich chain needs numpy")
    def test_the_real_chain_takes_the_pitch_and_keeps_the_padded_length(self) -> None:
        sidecar = self.sidecar(self.settings(fx="light"))
        self.assertEqual(sidecar.preset_st, -2.5)
        got = self.say(
            sidecar, "p", "Kneel. Now.", delivery={"emotion": "outrage", "intensity": 1.0}
        )
        first = base64.b64decode(got[0]["pcm"])
        self.assertEqual(len(first) // 2, 2400 + round(205 * SR / 1000))


if __name__ == "__main__":
    unittest.main()
