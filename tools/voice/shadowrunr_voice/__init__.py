# SPDX-License-Identifier: Apache-2.0
"""Malakor's voice sidecar: Kokoro through the operator's sherpa-onnx install, one sentence at a
time, with Oculus 15 mouth shapes from Kokoro's own durations (``docs/guide/voice.md``).

The daemon starts it (``python -m shadowrunr_voice``) and speaks JSON lines with it on stdio; the
protocol is ``docs/spec/contract.md`` appendix (voice).

- :mod:`.sidecar`: the protocol: ``hello``, then ``say`` and ``cancel`` in, ``audio`` and
  ``error`` out.
- :mod:`.engine`: one engine run per sentence, its arguments, its WAV, sherpa's pause cut.
- :mod:`.warm`: the warm engine, Kokoro kept loaded in one worker process per pool slot; only
  the worker imports sherpa_onnx and numpy.
- :mod:`.text`: where a line's sentences end.
- :mod:`.visemes`: token durations to mouth shapes; only its durations model needs numpy and
  onnxruntime, imported when it is built.

Ported from athena-portable's voice channel (``kokoro.py``, ``speech.py``, ``face.py`` and the
face helper's ``visemes.py``).
"""

__version__ = "0.1.0"
