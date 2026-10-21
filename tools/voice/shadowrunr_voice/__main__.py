# SPDX-License-Identifier: Apache-2.0
"""``python -m shadowrunr_voice --engine-dir <dir> [--durations <onnx>] --sid 3 --voice af_heart
[--render-timeout-ms N] [--workers N] [--threads N] [--engine auto|warm|process]``: the voice
sidecar, JSON lines on stdio (see :mod:`.sidecar`).

The protocol owns the process's real stdout. Anything else that prints, onnxruntime's native
logger included, goes to stderr at the file-descriptor level, so a library's chatter cannot
corrupt a line. The real stdin is moved off fd 0 the same way, so a library that asks the console
handle what it is never waits behind the reading thread's blocked read.
"""

from __future__ import annotations

import argparse
import os
import sys
from collections.abc import Iterable, Sequence
from pathlib import Path
from typing import IO

from .sidecar import DEFAULT_THREADS, DEFAULT_WORKERS, WARM_THREADS, Out, Settings, Sidecar, hello
from .warm import ENGINES


def _protocol_stream() -> IO[str]:
    """The real stdout for the protocol; fd 1 and ``sys.stdout`` become stderr from here on."""
    sys.stdout.flush()
    proto = os.fdopen(os.dup(1), "w", encoding="utf-8", newline="\n")
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    return proto


def _protocol_input() -> IO[bytes]:
    """The real stdin for the protocol; fd 0 and ``sys.stdin`` become the null device."""
    proto = os.fdopen(os.dup(0), "rb")
    null = os.open(os.devnull, os.O_RDONLY)
    os.dup2(null, 0)
    os.close(null)
    sys.stdin = open(os.devnull, encoding="utf-8")  # noqa: SIM115 - lives as long as the process
    return proto


def parse(argv: Sequence[str] | None = None) -> Settings:
    parser = argparse.ArgumentParser(prog="shadowrunr_voice", description=__doc__)
    parser.add_argument("--engine-dir", required=True, help="bin/ (the engine) and kokoro/")
    parser.add_argument("--durations", default=None, help="Kokoro's durations subgraph (.onnx)")
    parser.add_argument("--sid", type=int, default=3, help="the voice's verified speaker id")
    parser.add_argument("--voice", default="af_heart")
    parser.add_argument("--render-timeout-ms", type=int, default=30_000)
    parser.add_argument(
        "--workers", type=int, default=DEFAULT_WORKERS, help="engine runs at once (the pool)"
    )
    parser.add_argument(
        "--threads",
        type=int,
        default=None,
        help=f"threads per engine run (default {DEFAULT_THREADS}, {WARM_THREADS} when warm)",
    )
    parser.add_argument(
        "--engine",
        choices=ENGINES,
        default="auto",
        help="warm: Kokoro kept loaded per worker; process: the binary per sentence; "
        "auto: warm when sherpa_onnx is installed",
    )
    args = parser.parse_args(argv)
    return Settings(
        engine_dir=Path(args.engine_dir),
        sid=args.sid,
        voice=args.voice,
        timeout_s=max(args.render_timeout_ms, 1) / 1000,
        durations=Path(args.durations) if args.durations else None,
        workers=max(args.workers, 1),
        threads=max(args.threads, 1) if args.threads is not None else None,
        engine=args.engine,
    )


def run(settings: Settings, inbox: Iterable[bytes | str], out: Out) -> int:
    """Hello, then every line of ``inbox`` until it ends; then the sidecar stops."""
    message, dead, faces, warm = hello(settings)
    out.send(message)
    sidecar = Sidecar(settings, out.send, dead=dead, faces=faces, warm=warm)
    try:
        for raw in inbox:
            if raw.strip():
                sidecar.feed(raw)
    finally:
        sidecar.close()
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    settings = parse(argv)
    out = Out(_protocol_stream())
    return run(settings, _protocol_input(), out)


if __name__ == "__main__":
    raise SystemExit(main())
