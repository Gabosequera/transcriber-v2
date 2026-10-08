"""Physical >4GiB RF64 export through the real, SHA-pinned desktop.

Own V2 synthetic project, existing read-only tone, three tiny clips and silence
gaps. No playback, inference, new production code or policy changes.
"""
import copy
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import time
import uuid
import wave

ROOT = Path(__file__).resolve().parents[2]
EXE_SHA = "349df668c006a1565613ad6770edd918fb514243b5e2b32888469335f2837c36"
FLICKS = 705600000
DURATION = 22400
FRAMES = DURATION * 48000
CAP = 14 * 1024 ** 3
EXPECTED_PEAK = FRAMES * (8 + 4) + 16 * 1024 ** 2


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


class Limits(ctypes.Structure):
    _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t), ("MaximumWorkingSetSize", ctypes.c_size_t),
                ("ActiveProcessLimit", wintypes.DWORD), ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD), ("SchedulingClass", wintypes.DWORD)]


class IoCounters(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount", "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class ExtendedLimits(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", Limits), ("IoInfo", IoCounters), ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t), ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]


def own_job(process):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
    kernel.CreateJobObjectW.restype = wintypes.HANDLE
    kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel.CreateJobObjectW(None, None)
    limits = ExtendedLimits(); limits.BasicLimitInformation.LimitFlags = 0x2000
    if not handle or not kernel.SetInformationJobObject(handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)) or not kernel.AssignProcessToJobObject(handle, wintypes.HANDLE(int(process._handle))):
        error = ctypes.get_last_error()
        if handle:
            kernel.CloseHandle(handle)
        process.terminate()
        raise ctypes.WinError(error)
    return kernel, handle


def prepare(run):
    saved = ROOT / "implementation/evidence/local/prepared-20260930-133827-995706/escenario1.transcriptor/project.json"
    original = json.loads(saved.read_text(encoding="utf-8"))
    asset = copy.deepcopy(next(a for a in original["assets"] if a["name"] == "tone-220.wav"))
    tone = Path(asset["path"]).resolve()
    if tone != (ROOT / "tests/fixtures/media/tone-220.wav").resolve():
        raise ValueError("Only the existing V2 synthetic tone is permitted")
    sequence = copy.deepcopy(original["sequences"][0])
    audio = copy.deepcopy(next(t for t in sequence["tracks"] if t["kind"] == "audio"))
    audio.update(id="track-rf64-audio", muted=False, solo=False, gain_db=0.0)
    template = copy.deepcopy(next(c for c in sequence["clips"] if c["asset_id"] == asset["id"]))
    clips = []
    for index, position in enumerate([0.0, DURATION / 2, DURATION - 0.5]):
        clip = copy.deepcopy(template)
        clip.update(id=f"clip-rf64-{index}", track_id=audio["id"], source={"start": 0, "end": FLICKS // 2},
                    position=round(position * FLICKS), enabled=True, gain_db=0.0, audio_stream=0, name=f"Synthetic mark {index}")
        clip.pop("link_group", None); clip.pop("provenance", None)
        clips.append(clip)
    sequence.update(id="seq-rf64", name="Synthetic RF64 22400s", tracks=[audio], clips=clips, markers=[], sample_rate=48000)
    project = {"schema": "transcriptor-project/1", "project_id": f"proj-rf64-{uuid.uuid4().hex[:12]}", "name": "RF64 physical synthetic acceptance",
               "revision": 0, "assets": [asset], "sequences": [sequence], "active_sequence": sequence["id"],
               "layers": [], "layer_order": [], "settings": {"snapping": True, "skip_trims_on_play": False}}
    target = run / "synthetic.transcriptor/project.json"; target.parent.mkdir()
    target.write_text(json.dumps(project, separators=(",", ":")), encoding="utf-8")
    destination = run / "physical-rf64.wav"
    script = run / "rf64-script.json"
    script.write_text(json.dumps([{"op": "open", "path": target.as_posix()}, {"op": "assert", "clips": 3, "tracks": 1, "revision": 0},
        {"op": "set_in", "t": 0}, {"op": "set_out", "t": DURATION}, {"op": "log", "text": "RF64_EXPORT_BEGIN"},
        {"op": "export", "preset": "wav-pcm", "dest": destination.as_posix(), "range": True},
        {"op": "wait_export", "ms": 1200000}, {"op": "dump", "path": (run / "final-state.json").as_posix()},
        {"op": "assert", "clips": 3, "tracks": 1, "revision": 0}, {"op": "screenshot", "path": (run / "rf64-native.png").as_posix()},
        {"op": "quit"}], indent=2), encoding="utf-8")
    return tone, script, destination


def verify(path, tone):
    size = path.stat().st_size
    ds64 = None; fmt = None; data_offset = None
    with path.open("rb") as stream:
        first = stream.read(12)
        assert first == b"RF64\xff\xff\xff\xffWAVE", first
        while stream.tell() < 1024 * 1024:
            tag, length = struct.unpack("<4sI", stream.read(8))
            if tag == b"ds64":
                payload = stream.read(length)
                ds64 = struct.unpack("<QQQI", payload[:28])
            elif tag == b"fmt ":
                payload = stream.read(length)
                fmt = struct.unpack("<HHIIHH", payload[:16])
            elif tag == b"data":
                assert length == 0xffffffff
                data_offset = stream.tell(); break
            else:
                stream.seek(length, 1)
            if length % 2:
                stream.seek(1, 1)
        assert ds64 and fmt and data_offset
        assert ds64[:3] == (size - 8, FRAMES * 4, FRAMES), (ds64, size)
        assert size == data_offset + FRAMES * 4 and size > 2 ** 32
        assert fmt[0] in (1, 0xfffe) and fmt[1:] == (2, 48000, 192000, 4, 16), fmt
        with wave.open(str(tone), "rb") as source:
            assert (source.getnchannels(), source.getframerate(), source.getsampwidth()) == (2, 48000, 2)
            source.setpos(4800)
            expected = struct.unpack("<" + "h" * (9600 * 2), source.readframes(9600))
        readings = []
        for mark in [0.0, DURATION / 2, DURATION - 0.5]:
            offset = data_offset + round((mark + 0.1) * 48000) * 4
            stream.seek(offset)
            actual = struct.unpack("<" + "h" * len(expected), stream.read(len(expected) * 2))
            max_error = max(abs(a - b) for a, b in zip(actual, expected))
            rms = math.sqrt(sum(v * v for v in actual) / len(actual))
            assert max_error <= 2 and rms > 100, (mark, max_error, rms)
            readings.append({"timeline_seconds": mark + 0.1, "absolute_byte_offset": offset, "max_pcm16_error": max_error, "rms": rms})
        for position in [100, DURATION / 2 + 1, DURATION - 2]:
            stream.seek(data_offset + position * 48000 * 4)
            values = struct.unpack("<" + "h" * 4096, stream.read(8192))
            assert max(abs(v) for v in values) <= 1
            readings.append({"silence_seconds": position, "maximum_absolute_pcm16": max(abs(v) for v in values)})
        stream.seek(-1024, 2)
        assert len(stream.read(1024)) == 1024
    ffprobe = next((ROOT / "third-party").rglob("ffprobe.exe"))
    probe = json.loads(subprocess.check_output([str(ffprobe), "-v", "error", "-show_format", "-show_streams", "-of", "json", str(path)], creationflags=subprocess.CREATE_NO_WINDOW))
    assert abs(float(probe["format"]["duration"]) - DURATION) <= 1 / 48000
    return {"size": size, "ds64_riff_size": ds64[0], "ds64_data_size": ds64[1], "ds64_sample_count": ds64[2], "fmt": fmt,
            "data_offset": data_offset, "duration_seconds": DURATION, "readings": readings, "ffprobe": probe, "sha256": sha(path)}


def main():
    exe = ROOT / "target/release/Transcriptor.exe"
    assert sha(exe) == EXE_SHA, "Release changed; stop for coordination"
    free = shutil.disk_usage(ROOT).free
    assert EXPECTED_PEAK < CAP and free > CAP + 10 * 1024 ** 3, "Insufficient controlled budget/free disk"
    run = ROOT / "implementation/evidence/e2/continuacion-14" / datetime.now(timezone.utc).strftime("rf64-%Y%m%d-%H%M%S")
    run.mkdir()
    (run / ".gitignore").write_text("*.wav\n.export-*\n", encoding="utf-8")
    tone, script, destination = prepare(run)
    tone_sha = sha(tone)
    env = os.environ.copy()
    for name, folder in [("TRANSCRIPTOR_CONFIG_DIR", "config"), ("TRANSCRIPTOR_CACHE_DIR", "cache"), ("TRANSCRIPTOR_LOGS_DIR", "logs")]:
        env[name] = str(run / folder)
    started = time.perf_counter(); samples = []; failure = None
    print(json.dumps({"run": str(run), "budget_bytes": CAP, "expected_peak_bytes": EXPECTED_PEAK, "free_bytes": free, "duration": DURATION}), flush=True)
    with (run / "stdout.log").open("w") as stdout, (run / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(exe), "--script", str(script)], cwd=run, env=env, stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
        assert process.pid != 6868
        kernel, job = own_job(process)
        try:
            while process.poll() is None:
                elapsed = time.perf_counter() - started
                footprint = sum(path.stat().st_size for path in run.rglob("*") if path.is_file())
                samples.append({"seconds": elapsed, "bytes": footprint, "free_bytes": shutil.disk_usage(ROOT).free})
                if footprint >= CAP - 256 * 1024 ** 2 or elapsed > 1230:
                    failure = "own hard budget margin reached" if footprint >= CAP - 256 * 1024 ** 2 else "own export deadline exceeded"
                    break
                if len(samples) % 30 == 0:
                    print(json.dumps(samples[-1]), flush=True)
                time.sleep(0.5)
        finally:
            kernel.CloseHandle(job)  # Kill-on-close, only this own app tree.
        process.wait(timeout=15)
    log = script.with_suffix(".log").read_text(encoding="utf-8", errors="replace")
    report = {"result": "FAIL", "run": str(run), "exe_sha256": EXE_SHA, "pid": process.pid, "protected_human_pid": 6868,
              "free_before": free, "budget_bytes": CAP, "expected_peak_bytes": EXPECTED_PEAK, "peak_run_bytes": max(s["bytes"] for s in samples),
              "elapsed_seconds": time.perf_counter() - started, "exit_code": process.returncode, "failure": failure,
              "source_sha256": tone_sha, "source_unchanged": sha(tone) == tone_sha, "duration_seconds": DURATION}
    if failure is None and process.returncode == 0 and "guion terminado; fallos=false" in log and destination.is_file():
        try:
            report["verification"] = verify(destination, tone)
            report["result"] = "PASS" if report["source_unchanged"] else "FAIL_SOURCE_CHANGED"
        except Exception as error:
            report["failure"] = repr(error)
    (run / "measurements.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    (run / "footprint-samples.json").write_text(json.dumps(samples, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2), flush=True)
    raise SystemExit(0 if report["result"] == "PASS" else 1)


if __name__ == "__main__":
    main()
