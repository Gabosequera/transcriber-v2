"""Run the new editorial host inside an 8 GiB owned Windows job, no UI control."""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
CAP = 8 * 1024 ** 3


class Limits(ctypes.Structure):
    _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t), ("MaximumWorkingSetSize", ctypes.c_size_t),
                ("ActiveProcessLimit", wintypes.DWORD), ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD), ("SchedulingClass", wintypes.DWORD)]


class IoCounters(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount", "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class ExtendedLimits(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", Limits), ("IoInfo", IoCounters), ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t), ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("--kind", choices=("topics", "layers", "trims", "montage"), default="topics")
    options = parser.parse_args()
    output = options.output.resolve()
    fixture = options.fixture.resolve()
    if not output.is_relative_to(ROOT / ".local") or output.exists() or not fixture.is_relative_to(ROOT / ".local"):
        raise ValueError("Use a fresh owned evidence directory and existing synthetic V2 project")
    output.mkdir(parents=True)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
    kernel.CreateJobObjectW.restype = wintypes.HANDLE
    kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    kernel.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
    kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel.CreateJobObjectW(None, None)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    limits = ExtendedLimits()
    limits.BasicLimitInformation.LimitFlags = 0x2000 | 0x200  # kill-on-close + job memory cap
    limits.JobMemoryLimit = CAP
    process = None
    host = ROOT / "target/debug/examples" / ("editorial_host.exe" if options.kind == "topics" else "editorial_actions_host.exe")
    worker = ROOT / "workers/python/editorial_worker.py"
    def digest(path):
        with path.open("rb") as source:
            return hashlib.file_digest(source, "sha256").hexdigest()
    report = {"job_memory_limit": CAP, "fixture_asr_invented": True, "gui_exercised": False, "supervisor_handshake": False,
              "kind": options.kind}
    start = time.monotonic()
    peak = 0
    try:
        report.update(host_sha256=digest(host), worker_sha256=digest(worker), supervisor_sha256=digest(Path(__file__)))
        if not kernel.SetInformationJobObject(handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            raise ctypes.WinError(ctypes.get_last_error())
        with (output / "stdout.log").open("wb") as stdout, (output / "stderr.log").open("wb") as stderr:
            command = [str(host), str(ROOT), str(output / "run"), str(fixture)]
            if options.kind != "topics":
                command.append(options.kind)
            process = subprocess.Popen(command,
                                       stdin=subprocess.PIPE, stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
            if not kernel.AssignProcessToJobObject(handle, wintypes.HANDLE(int(process._handle))):
                error = ctypes.get_last_error()
                process.terminate()  # This exact host is still waiting for the handshake and owns no worker.
                process.wait(timeout=10)
                raise ctypes.WinError(error)
            report.update(pid=process.pid)
            process.stdin.write(b"owned-job-ready\n")
            process.stdin.close()
            report["supervisor_handshake"] = True
            while process.poll() is None:
                sample = ExtendedLimits()
                if not kernel.QueryInformationJobObject(handle, 9, ctypes.byref(sample), ctypes.sizeof(sample), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                peak = max(peak, sample.PeakJobMemoryUsed)
                if time.monotonic() - start > 1400:
                    raise TimeoutError("Owned editorial host exceeded 1400 seconds")
                time.sleep(0.2)
            sample = ExtendedLimits()
            if not kernel.QueryInformationJobObject(handle, 9, ctypes.byref(sample), ctypes.sizeof(sample), None):
                raise ctypes.WinError(ctypes.get_last_error())
            peak = max(peak, sample.PeakJobMemoryUsed)
            report.update(exit_code=process.returncode, peak_job_memory=peak)
            if process.returncode:
                raise RuntimeError("Editorial host failed; preserve stdout/stderr and job evidence")
            if peak > CAP:
                raise RuntimeError("Editorial host exceeded the measured memory budget")
            if digest(worker) != report["worker_sha256"] or digest(host) != report["host_sha256"]:
                raise RuntimeError("Editorial host/worker changed during test")
            report["accepted"] = True
    except Exception as error:
        report.update(accepted=False, error=str(error))
        raise
    finally:
        report["peak_job_memory"] = peak
        kernel.CloseHandle(handle)
        cleanup_error = None
        if process is not None:
            try:
                process.wait(timeout=10)
                report["own_process_signaled"] = True
            except subprocess.TimeoutExpired as error:
                cleanup_error = error
                report.update(accepted=False, own_process_signaled=False, cleanup_error=str(error))
        try:
            report.update(worker_sha256_after=digest(worker), host_sha256_after=digest(host))
        except OSError as error:
            report["final_hash_error"] = str(error)
        report["duration_seconds"] = time.monotonic() - start
        (output / "supervisor.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(json.dumps(report), flush=True)
        if cleanup_error is not None:
            raise RuntimeError("Owned host did not signal after closing its JobObject; evidence preserved") from cleanup_error


if __name__ == "__main__":
    main()
