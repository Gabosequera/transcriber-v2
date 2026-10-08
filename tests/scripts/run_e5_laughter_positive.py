"""Bounded standalone author-labelled laughter smoke; --prepare-only never runs ML.

No ASR transcript, V1 import or project mutation. Human temporal ground truth
remains pending even when the detector returns a valid positive event.
"""
from __future__ import annotations
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
import wave

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / ".local/e5-laughter-positive-fixture-01"
CAP = 8 * 1024**3
DEADLINE = 240
SOURCE_SHA = "85569b4951cc0127a13cbea44dd1cd698beab0db68e0167b699196eadf2fa502"
PCM_SHA = "3ac02b42fe6d0a7e1e726a4d5b69323da426df16afb73ad329b12148be597380"
MODEL_SHA = "449b14f73c70db26da9b4a59ee77d9a9b29fbcaceb083dd7ea27cdfaa68442a0"
CONFIG_SHA = "ffcc5c417fe11433447975d5053b2279fbeafd6bca03dd2753082e72ad2d36b7"


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True, allow_nan=False).encode()


def sha(path):
    with Path(path).open("rb") as source:
        digest = hashlib.sha256()
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def plan():
    source = FIXTURE / "small-group-laughter-freesound-preview-hq.mp3"
    pcm = FIXTURE / "small-group-laughter-mono-16000.wav"
    if source.stat().st_size != 111408 or sha(source) != SOURCE_SHA or sha(pcm) != PCM_SHA:
        raise ValueError("Public fixture bytes changed")
    with wave.open(str(pcm), "rb") as audio:
        if (audio.getnchannels(), audio.getframerate(), audio.getsampwidth(), audio.getcomptype()) != (1, 16000, 2, "NONE"):
            raise ValueError("Expected normalized mono PCM16 16kHz")
        frames = audio.getnframes()
    if not 0 < frames <= 16000 * 30 or pcm.stat().st_size > 1024**2:
        raise ValueError("PCM fixture exceeds bounds")
    manifest = ROOT / ".local/models/laughter-omine/model-manifest.json"
    resources = json.loads(manifest.read_bytes())
    for name, expected in (("model.safetensors", MODEL_SHA), ("config.json", CONFIG_SHA)):
        if resources["files"][name]["sha256"] != expected:
            raise ValueError("Model manifest differs from existing laughter profile")
    identity = {"fixture": "Freesound15294 official author-labelled preview", "source_sha256": SOURCE_SHA,
                "normalized_audio_sha256": PCM_SHA, "asr_exercised": False}
    params = {"job_id": "public-laughter-positive", "project_id": "public-laughter-fixture", "revision": 0,
              "project_digest": hashlib.sha256(canonical(identity)).hexdigest(), "asset_id": "freesound-15294", "track_id": "a0",
              "audio_index": 0, "audio_offset_ticks": 0, "source_sha256": SOURCE_SHA,
              "source_duration_ticks": frames * 44100, "timebase": "flicks/705600000",
              "normalized_audio_path": str(pcm), "normalized_audio_sha256": PCM_SHA,
              "model_path": str(manifest.with_name("model.safetensors")), "config_path": str(manifest.with_name("config.json")),
              "manifest_path": str(manifest), "model_sha256": MODEL_SHA, "config_sha256": CONFIG_SHA,
              "parameters": {"device": "cpu", "threads": 2, "threshold": .5, "amplitude_boost": True,
                             "min_dur": .2, "merge_gap": .2, "input_sec": 7, "overlap_sec": 2, "batch_size": 1}}
    return {"schema": "tv2-laughter-positive-smoke-plan/1", "parameters": params,
            "pcm_frames": frames, "pcm_duration_seconds": frames / 16000,
            "source_duration_origin": "normalized PCM clock, T0; source MP3 duration recorded separately",
            "reference": {"kind": "author_label_clip_level", "author": "Ch0cchi", "label": "small group laughter",
                          "source": "https://freesound.org/people/Ch0cchi/sounds/15294/", "license": "CC-BY-3.0",
                          "human_listening_executed": False, "temporal_ground_truth_verified": False, "intervals": []},
            "acceptance": {"technical_detection_requires_valid_event": True,
                           "full_positive_acceptance_pending_human_annotation": True},
            "worker_sha256": sha(ROOT / "workers/python/laughter_worker.py"), "model_manifest_sha256": sha(manifest),
            "client_helper_sha256": sha(ROOT / "tests/scripts/test_e5_laughter.py"),
            "runtime_launcher_sha256": sha(ROOT / ".local/e5-laughter-venv/Scripts/python.exe"),
            "runtime_lock_sha256": sha(ROOT / "workers/python/requirements-laughter.lock"),
            "cap_bytes": CAP, "deadline_seconds": DEADLINE, "asr_exercised": False, "gui_exercised": False}


def child(work_root, handshake_only=False):
    # Parent assigns launcher and actual interpreter before this process may
    # launch the worker or import the existing test client.
    print("TV2_LAUGHTER_POSITIVE_READY " + str(os.getpid()), flush=True)
    if sys.stdin.buffer.readline(64) != b"GO\n":
        raise RuntimeError("Owned supervisor handshake absent")
    if handshake_only:
        print(canonical({"phase": "handshake_only_completed", "pid": os.getpid(), "ml_executed": False}).decode(), flush=True)
        return 0
    frozen = json.loads((FIXTURE / "positive-smoke-plan.json").read_bytes())
    if plan() != frozen:
        raise RuntimeError("Fixture/resources/helper changed since preparation")
    spec = importlib.util.spec_from_file_location("existing_laughter_client", ROOT / "tests/scripts/test_e5_laughter.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    client = module.Client(work_root)
    try:
        client.send("hello", "hello")
        hello = client.reply("hello", timeout=30)
        if "result" not in hello or hello["result"]["runtime"]["python"] != "3.11.15":
            raise RuntimeError("Existing laughter runtime handshake failed")
        client.send("positive", "run", frozen["parameters"])
        reply = client.reply("positive", timeout=DEADLINE - 30)
        if "result" not in reply:
            raise RuntimeError("Laughter worker rejected positive fixture: " + repr(reply))
        receipt = reply["result"]
        output = work_root / frozen["parameters"]["job_id"] / "laughter/events.json"
        if sha(output) != receipt["artifacts"]["laughter/events.json"]:
            raise RuntimeError("Result artifact digest differs")
        result = json.loads(output.read_bytes())
        events = result["events"]
        duration = frozen["pcm_duration_seconds"]
        for event in events:
            values = [event[name] for name in ("t_ini", "t_fin", "dur", "conf", "mean_conf", "max_conf")]
            if not all(type(value) in (int, float) and math.isfinite(value) for value in values):
                raise RuntimeError("Invalid event numbers")
            if not (0 <= event["t_ini"] < event["t_fin"] <= duration and event["dur"] >= .2
                    and .5 - .000501 <= event["mean_conf"] <= event["max_conf"] <= 1
                    and event["track_id"] == "a0" and event["tipo"] == "laughter"):
                raise RuntimeError("Event differs from fixed bounds/profile")
        if (result["laughter"]["execution_state"] != "completed" or result["project_digest"] != frozen["parameters"]["project_digest"]
                or result["laughter"]["parameters"] != frozen["parameters"]["parameters"] or plan() != frozen):
            raise RuntimeError("Completed output/input lineage differs")
        summary = {"schema": "tv2-laughter-positive-smoke-result/1", "receipt": receipt, "events": events,
                   "technical_detection_present": bool(events), "native_laughter_inference_executed": True,
                   "full_positive_acceptance_complete": False, "human_listening_executed": False,
                   "temporal_ground_truth_verified": False, "asr_exercised": False, "gui_exercised": False}
        (work_root / "technical-result.json").write_bytes(canonical(summary) + b"\n")
        print(canonical(summary).decode(), flush=True)
        return 0 if events else 1
    finally:
        client.close()


class BasicLimits(ctypes.Structure):
    _fields_ = [("process_time", ctypes.c_int64), ("job_time", ctypes.c_int64), ("flags", wintypes.DWORD),
                ("min_ws", ctypes.c_size_t), ("max_ws", ctypes.c_size_t), ("active", wintypes.DWORD),
                ("affinity", ctypes.c_size_t), ("priority", wintypes.DWORD), ("scheduling", wintypes.DWORD)]


class IoCounters(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("read_ops", "write_ops", "other_ops", "read_bytes", "write_bytes", "other_bytes")]


class ExtendedLimits(ctypes.Structure):
    _fields_ = [("basic", BasicLimits), ("io", IoCounters), ("process_memory", ctypes.c_size_t),
                ("job_memory", ctypes.c_size_t), ("peak_process", ctypes.c_size_t), ("peak_job", ctypes.c_size_t)]


class ThreadEntry(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("usage", wintypes.DWORD), ("thread_id", wintypes.DWORD),
                ("owner_pid", wintypes.DWORD), ("base_priority", wintypes.LONG),
                ("delta_priority", wintypes.LONG), ("flags", wintypes.DWORD)]


def resume_own_primary(kernel, pid):
    # Popen closes CreateProcessW's primary thread handle. A suspended launcher
    # cannot create other threads; locate exactly its one own primary thread.
    snapshot = kernel.CreateToolhelp32Snapshot(4, 0)
    if snapshot == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    entry = ThreadEntry()
    entry.size = ctypes.sizeof(entry)
    matches = []
    try:
        more = kernel.Thread32First(snapshot, ctypes.byref(entry))
        while more:
            if entry.owner_pid == pid:
                matches.append(entry.thread_id)
            more = kernel.Thread32Next(snapshot, ctypes.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    if len(matches) != 1:
        raise RuntimeError("Suspended owned launcher must have exactly one primary thread")
    handle = kernel.OpenThread(0x0002, False, matches[0])  # THREAD_SUSPEND_RESUME only
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        previous = kernel.ResumeThread(handle)
        if previous == 0xFFFFFFFF:
            raise ctypes.WinError(ctypes.get_last_error())
        if previous != 1:
            raise RuntimeError("Unexpected owned primary suspend count")
    finally:
        kernel.CloseHandle(handle)
    return matches[0]


def supervise(work_root, evidence, handshake_only=False, handshake_eof=False):
    if os.name != "nt" or work_root.exists() or evidence.exists() or not work_root.is_relative_to(ROOT) or not evidence.is_relative_to(ROOT):
        raise ValueError("Fresh V2 work/evidence paths on Windows required")
    frozen = json.loads((FIXTURE / "positive-smoke-plan.json").read_bytes())
    if plan() != frozen:
        raise ValueError("Prepared resources changed")
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    signatures = {"CreateJobObjectW": ([ctypes.c_void_p, wintypes.LPCWSTR], wintypes.HANDLE),
                  "SetInformationJobObject": ([wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD], wintypes.BOOL),
                  "QueryInformationJobObject": ([wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p], wintypes.BOOL),
                  "AssignProcessToJobObject": ([wintypes.HANDLE, wintypes.HANDLE], wintypes.BOOL),
                  "IsProcessInJob": ([wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.BOOL)], wintypes.BOOL),
                  "CreateToolhelp32Snapshot": ([wintypes.DWORD, wintypes.DWORD], wintypes.HANDLE),
                  "Thread32First": ([wintypes.HANDLE, ctypes.POINTER(ThreadEntry)], wintypes.BOOL),
                  "Thread32Next": ([wintypes.HANDLE, ctypes.POINTER(ThreadEntry)], wintypes.BOOL),
                  "OpenThread": ([wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE),
                  "ResumeThread": ([wintypes.HANDLE], wintypes.DWORD),
                  "OpenProcess": ([wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE),
                  "CloseHandle": ([wintypes.HANDLE], wintypes.BOOL)}
    for name, (arguments, result) in signatures.items():
        function = getattr(kernel, name)
        function.argtypes, function.restype = arguments, result
    job = kernel.CreateJobObjectW(None, None)
    if not job:
        raise ctypes.WinError(ctypes.get_last_error())
    limits = ExtendedLimits()
    active_cap, active_deadline = (1024**3, 30) if handshake_only else (CAP, DEADLINE)
    limits.basic.flags, limits.job_memory = 0x2000 | 0x200, active_cap
    process = None
    stdout = stderr = None
    read_errors = []
    peak = 0
    started = time.monotonic()
    report = {"schema": "tv2-laughter-positive-supervisor/1", "cap_bytes": active_cap, "deadline_seconds": active_deadline,
              "owned_handshake": False, "technical_detection_present": False, "full_positive_acceptance_complete": False,
              "human_listening_executed": False, "asr_exercised": False, "handshake_only": handshake_only,
              "eof_cleanup_probe": handshake_eof,
              "native_inference_requested": not handshake_only, "script_sha256": sha(__file__)}
    work_root.mkdir(parents=True)
    try:
        if not kernel.SetInformationJobObject(job, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            raise ctypes.WinError(ctypes.get_last_error())
        env = {key: value for key, value in os.environ.items() if key.upper() in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP", "PATH")}
        env.update(HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", OMP_NUM_THREADS="2", MKL_NUM_THREADS="2",
                   TOKENIZERS_PARALLELISM="false", PYTHONDONTWRITEBYTECODE="1")
        stderr = (work_root / "stderr.log").open("wb")
        child_command = [sys.executable, "-I", "-B", str(Path(__file__).resolve()), "--child", "--work-root", str(work_root)]
        if handshake_only:
            child_command.append("--handshake-only")
        process = subprocess.Popen(child_command,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, env=env,
                                   creationflags=subprocess.CREATE_NO_WINDOW | 0x00000004)  # CREATE_SUSPENDED
        report.update(launcher_pid=process.pid, launcher_created_suspended=True)
        def membership(handle, phase):
            member = wintypes.BOOL()
            report["ownership_phase"] = phase
            if not kernel.IsProcessInJob(handle, job, ctypes.byref(member)):
                raise ctypes.WinError(ctypes.get_last_error())
            report[phase] = bool(member.value)
            return bool(member.value)
        launcher = wintypes.HANDLE(int(process._handle))
        if not membership(launcher, "launcher_member_before_assign"):
            report["ownership_phase"] = "assign_launcher"
            if not kernel.AssignProcessToJobObject(job, launcher):
                raise ctypes.WinError(ctypes.get_last_error())
        if not membership(launcher, "launcher_member_after_assign"):
            raise RuntimeError("Launcher outside owned memory cap")
        report["ownership_phase"] = "resume_own_primary"
        report["own_primary_thread_id"] = resume_own_primary(kernel, process.pid)
        ready = []
        reader = threading.Thread(target=lambda: ready.append(process.stdout.readline(256)), daemon=True)
        reader.start()
        reader.join(timeout=30)
        if not ready or not ready[0].startswith(b"TV2_LAUGHTER_POSITIVE_READY "):
            raise RuntimeError("Native child supervisor handshake missing")
        actual_pid = int(ready[0].split()[1])
        report["native_supervisor_pid"] = actual_pid
        if actual_pid != process.pid:
            report["ownership_phase"] = "open_actual_setquota_terminate_query"
            # Assign requires SET_QUOTA|TERMINATE; membership additionally
            # requires QUERY_LIMITED_INFORMATION. Never omit the actual cap.
            actual = kernel.OpenProcess(0x0100 | 0x0001 | 0x1000, False, actual_pid)
            if not actual:
                raise ctypes.WinError(ctypes.get_last_error())
            try:
                if not membership(actual, "actual_member_before_assign"):
                    report["ownership_phase"] = "assign_actual"
                    if not kernel.AssignProcessToJobObject(job, actual):
                        raise ctypes.WinError(ctypes.get_last_error())
                    report["actual_assignment_required"] = True
                else:
                    report["actual_assignment_required"] = False
                if not membership(actual, "actual_member_after_assign"):
                    raise RuntimeError("Actual interpreter outside owned memory cap")
            finally:
                kernel.CloseHandle(actual)
        report.update(owned_handshake=True, ownership_phase="owned_before_GO")
        stdout = (work_root / "stdout.jsonl").open("wb")
        def drain():
            try:
                copy_output(process.stdout, stdout)
            except Exception as error:
                read_errors.append(error)
        reader = threading.Thread(target=drain, daemon=True)
        reader.start()
        if handshake_eof:
            process.stdin.close()
        else:
            process.stdin.write(b"GO\n")
            process.stdin.flush()
        while process.poll() is None:
            if read_errors:
                raise read_errors[0]
            if time.monotonic() - started >= active_deadline:
                raise TimeoutError("Owned laughter positive deadline")
            if (work_root / "stderr.log").stat().st_size > 1024**2:
                raise RuntimeError("Owned stderr exceeds1MiB")
            if not kernel.QueryInformationJobObject(job, 9, ctypes.byref(limits), ctypes.sizeof(limits), None):
                raise ctypes.WinError(ctypes.get_last_error())
            peak = max(peak, limits.peak_job)
            time.sleep(.1)
        reader.join(timeout=5)
        if reader.is_alive():
            raise RuntimeError("Owned stdout reader did not finish")
        if read_errors:
            raise read_errors[0]
        stdout.close()
        stderr.close()
        if not kernel.QueryInformationJobObject(job, 9, ctypes.byref(limits), ctypes.sizeof(limits), None):
            raise ctypes.WinError(ctypes.get_last_error())
        peak = max(peak, limits.peak_job)
        report.update(exit_code=process.returncode, own_process_signaled=True)
        result = work_root / "technical-result.json"
        if result.exists():
            report["technical_detection_present"] = json.loads(result.read_bytes())["technical_detection_present"]
        if handshake_eof:
            if process.returncode != 1:
                raise RuntimeError("EOF must reject before ML with exit1")
            report["expected_eof_rejection_observed"] = True
        elif process.returncode or (not handshake_only and not report["technical_detection_present"]) or peak > active_cap:
            raise RuntimeError("Technical author-labelled positive smoke failed; preserve evidence")
    except Exception as error:
        report["error"] = str(error)
    finally:
        if kernel.QueryInformationJobObject(job, 9, ctypes.byref(limits), ctypes.sizeof(limits), None):
            peak = max(peak, limits.peak_job)
        kernel.CloseHandle(job)  # kill only the owned tree, also after timeout/failure
        if process is not None:
            # Before GO, EOF also terminates an interpreter that could not be
            # attached to the owned job. Never leave that handshake waiting.
            process.stdin.close()
            if process.poll() is None:
                process.wait(timeout=10)
            report.update(exit_code=process.returncode, own_process_signaled=True)
        if stdout is not None:
            stdout.close()
        if stderr is not None:
            stderr.close()
        report.update(peak_job_memory=peak, elapsed_seconds=time.monotonic() - started)
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.write_bytes(canonical(report) + b"\n")
        print(canonical(report).decode(), flush=True)
    return 1 if "error" in report else 0


def copy_output(source, target):
    count = 0
    for line in iter(lambda: source.readline(1024**2 + 1), b""):
        count += len(line)
        if len(line) > 1024**2 or count > 1024**2:
            raise RuntimeError("Owned stdout exceeds 1MiB")
        target.write(line)
        target.flush()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--child", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--handshake-only", action="store_true", help="stdlib ownership micro; never imports ML/client")
    parser.add_argument("--handshake-eof", action="store_true", help="stdlib EOF rejection/cleanup; requires handshake-only")
    parser.add_argument("--work-root", type=Path)
    parser.add_argument("--evidence", type=Path)
    options = parser.parse_args()
    if options.handshake_eof and not options.handshake_only:
        parser.error("EOF diagnostic requires --handshake-only")
    if options.prepare_only:
        target = FIXTURE / "positive-smoke-plan.json"
        if target.exists():
            raise ValueError("Preserve already prepared plan")
        prepared = plan()
        target.write_bytes(canonical(prepared) + b"\n")
        print(canonical(prepared).decode())
        return 0
    if options.work_root is None:
        parser.error("--work-root required")
    if options.child:
        return child(options.work_root.resolve(), options.handshake_only)
    if options.evidence is None:
        parser.error("--evidence required")
    return supervise(options.work_root.resolve(), options.evidence.resolve(), options.handshake_only, options.handshake_eof)


if __name__ == "__main__":
    raise SystemExit(main())
