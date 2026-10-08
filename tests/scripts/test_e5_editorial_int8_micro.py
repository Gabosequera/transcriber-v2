"""Bounded native route diagnostic, random miniature Qwen2, never pretrained weights."""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
CAP = 1024 ** 3


def child():
    if sys.stdin.buffer.readline() != b"owned-job-ready\n":
        raise ValueError("Missing supervisor handshake")
    import copy
    import torch
    from transformers import Qwen2Config, Qwen2ForCausalLM
    torch.set_num_threads(2)
    torch.set_num_interop_threads(1)
    torch.manual_seed(11)
    config = Qwen2Config(vocab_size=128, hidden_size=64, intermediate_size=128,
                         num_hidden_layers=2, num_attention_heads=4,
                         num_key_value_heads=2, max_position_embeddings=64,
                         tie_word_embeddings=True, bos_token_id=1, eos_token_id=2,
                         pad_token_id=0, attn_implementation="eager")
    model = Qwen2ForCausalLM(config).bfloat16().eval()
    baseline = copy.deepcopy(model).float().eval()  # Only this tiny random baseline.
    inputs = torch.tensor([[1, 9, 14, 31, 27]], dtype=torch.long)
    with torch.inference_mode():
        expected = baseline(inputs).logits
    converted = []

    def convert(parent, prefix=""):
        # Store names only; no global old module/weight references.
        for name in tuple(parent._modules):
            module = parent._modules[name]
            path = prefix + name
            if type(module) is torch.nn.Linear:
                module.float()
                module.qconfig = torch.ao.quantization.default_dynamic_qconfig
                quantized = torch.ao.nn.quantized.dynamic.Linear.from_float(module)
                parent._modules[name] = quantized
                converted.append(path)
                del module, quantized
            else:
                convert(module, path + ".")

    convert(model)
    model.float().eval()  # Remaining embedding, norms and buffers only.
    with torch.inference_mode():
        actual = model(inputs).logits
        tokens = model.generate(inputs, max_new_tokens=4, do_sample=False,
                                pad_token_id=0, eos_token_id=None)
    max_error = (actual - expected).abs().max().item()
    assert torch.isfinite(actual).all() and max_error < 0.15
    assert len(converted) == 15 and tokens.shape == (1, 9)
    assert model.model.embed_tokens.weight.dtype is torch.float32
    assert model.lm_head.weight().dtype is torch.qint8
    print(json.dumps({"accepted": True, "pretrained_model_executed": False,
                      "random_miniature_architecture": "Qwen2ForCausalLM",
                      "torch": torch.__version__, "engine": torch.backends.quantized.engine,
                      "threads": torch.get_num_threads(), "converted_linears": len(converted),
                      "max_abs_logit_error": max_error, "logits_dtype": str(actual.dtype),
                      "generated_token_count": tokens.shape[1] - inputs.shape[1],
                      "generated_random_token_ids": tokens.tolist()[0]}, sort_keys=True), flush=True)


class Limits(ctypes.Structure):
    _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
                ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wintypes.DWORD),
                ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD),
                ("SchedulingClass", wintypes.DWORD)]


class IO(ctypes.Structure):
    _fields_ = [(n, ctypes.c_uint64) for n in ("ReadOperationCount", "WriteOperationCount",
                "OtherOperationCount", "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class Extended(ctypes.Structure):
    _fields_ = [("Basic", Limits), ("Io", IO), ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t), ("PeakProcessMemoryUsed", ctypes.c_size_t),
                ("PeakJobMemoryUsed", ctypes.c_size_t)]


def supervise():
    output = Path(sys.argv[1]).resolve()
    if not output.is_relative_to(ROOT / ".local") or output.exists():
        raise ValueError("Fresh owned evidence directory required")
    output.mkdir(parents=True)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
    kernel.CreateJobObjectW.restype = wintypes.HANDLE
    kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    kernel.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
    kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    kernel.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel.CreateJobObjectW(None, None)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    limits = Extended()
    limits.Basic.LimitFlags = 0x2000 | 0x200
    limits.JobMemoryLimit = CAP
    process = None
    start = time.monotonic()
    peak = 0
    report = {"cap_bytes": CAP, "deadline_seconds": 60, "accepted": False,
              "pretrained_model_executed": False}
    try:
        if not kernel.SetInformationJobObject(handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            raise ctypes.WinError(ctypes.get_last_error())
        env = dict(os.environ, HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1",
                   OMP_NUM_THREADS="2", MKL_NUM_THREADS="2", PYTHONDONTWRITEBYTECODE="1")
        with (output / "stdout.log").open("wb") as stdout, (output / "stderr.log").open("wb") as stderr:
            process = subprocess.Popen([sys.executable, "-I", "-B", str(Path(__file__).resolve()), "--child"],
                                       stdin=subprocess.PIPE, stdout=stdout, stderr=stderr,
                                       env=env, creationflags=subprocess.CREATE_NO_WINDOW)
            if not kernel.AssignProcessToJobObject(handle, wintypes.HANDLE(int(process._handle))):
                process.terminate()  # Exact owned process before imports.
                process.wait(timeout=5)
                raise ctypes.WinError(ctypes.get_last_error())
            report["pid"] = process.pid
            process.stdin.write(b"owned-job-ready\n")
            process.stdin.close()
            while process.poll() is None:
                sample = Extended()
                if not kernel.QueryInformationJobObject(handle, 9, ctypes.byref(sample), ctypes.sizeof(sample), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                peak = max(peak, sample.PeakJobMemoryUsed)
                if time.monotonic() - start > 60:
                    raise TimeoutError("Own microtest deadline")
                time.sleep(0.05)
            sample = Extended()
            if kernel.QueryInformationJobObject(handle, 9, ctypes.byref(sample), ctypes.sizeof(sample), None):
                peak = max(peak, sample.PeakJobMemoryUsed)
            report["exit_code"] = process.returncode
            if process.returncode:
                raise RuntimeError("Microtest failed; preserve logs")
            report["accepted"] = True
    except Exception as error:
        report["error"] = type(error).__name__ + ": " + str(error)
        raise
    finally:
        if process is not None and process.poll() is None:
            kernel.TerminateJobObject(handle, 124)  # Only this job and exact owned descendants.
            process.wait(timeout=5)
        kernel.CloseHandle(handle)
        report.update(peak_job_memory=peak, duration_seconds=time.monotonic() - start,
                      own_process_signaled=process is not None and process.poll() is not None)
        (output / "supervisor.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(json.dumps(report), flush=True)


if __name__ == "__main__":
    child() if sys.argv[1] == "--child" else supervise()
