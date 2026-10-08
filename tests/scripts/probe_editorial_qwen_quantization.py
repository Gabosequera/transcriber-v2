"""Four bounded real-checkpoint forwards, no generation or editorial acceptance."""
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
MODEL = ROOT / ".local/models/qwen2.5-1.5b-instruct"
CAP = 8 * 1024 ** 3


def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def emit(value):
    print(json.dumps(value, ensure_ascii=False, sort_keys=True, allow_nan=False), flush=True)


def child():
    if sys.stdin.buffer.readline() != b"owned-job-ready\n":
        raise ValueError("Supervisor handshake missing")
    import importlib.metadata as metadata
    manifest = json.loads((MODEL / "model-manifest.json").read_text())
    for name, entry in manifest["files"].items():
        assert (MODEL / name).stat().st_size == entry["size"] and sha(MODEL / name) == entry["sha256"]
    lock = ROOT / "workers/python/requirements-editorial.lock"
    dependencies = dict(line.split("==") for line in lock.read_text().splitlines() if line.strip())
    assert all(metadata.version(name) == version for name, version in dependencies.items())
    emit({"phase": "resources_verified", "model_manifest_sha256": sha(MODEL / "model-manifest.json"),
          "lock_sha256": sha(lock), "dependencies": dependencies})
    import torch
    from transformers import AutoTokenizer, AutoModelForCausalLM
    torch.set_num_threads(2)
    torch.set_num_interop_threads(1)
    torch.manual_seed(0)
    assert "x86" in torch.backends.quantized.supported_engines
    torch.backends.quantized.engine = "x86"
    tokenizer = AutoTokenizer.from_pretrained(MODEL, local_files_only=True, trust_remote_code=False)
    messages = [{"role": "system", "content": "JSON only."},
                {"role": "user", "content": 'Devuelve {"tema":"saludo"}.'}]
    inputs = tokenizer.apply_chat_template(messages, tokenize=True, add_generation_prompt=True,
                                           return_tensors="pt")
    assert inputs.shape[1] <= 32, "Keep complete chat template within 32 tokens"
    emit({"phase": "prompt", "messages": messages, "input_tokens": inputs.shape[1],
          "input_ids": inputs.tolist()[0], "eos": json.loads((MODEL / "generation_config.json").read_text())["eos_token_id"]})
    started = time.monotonic()
    model = AutoModelForCausalLM.from_pretrained(MODEL, local_files_only=True, trust_remote_code=False,
                                               use_safetensors=True, torch_dtype=torch.bfloat16,
                                               attn_implementation="eager", low_cpu_mem_usage=True).eval()
    emit({"phase": "loaded_bf16", "seconds": time.monotonic() - started,
          "tied_head_embedding": model.lm_head.weight is model.model.embed_tokens.weight})
    baseline = None

    def forward(phase):
        nonlocal baseline
        started = time.monotonic()
        with torch.inference_mode():
            output = model(input_ids=inputs, attention_mask=torch.ones_like(inputs),
                           use_cache=True, num_logits_to_keep=1)
        actual = output.logits[0, -1].float().clone()
        assert torch.isfinite(actual).all()
        if baseline is None:
            baseline = actual.clone()
        values, indices = actual.topk(10)
        delta = actual - baseline
        cosine = torch.nn.functional.cosine_similarity(actual, baseline, dim=0).item()
        cache = output.past_key_values
        cache_layers = len(cache)
        cache_tokens = cache.get_seq_length() if hasattr(cache, "get_seq_length") else cache[0][0].shape[-2]
        emit({"phase": phase, "seconds": time.monotonic() - started, "finite": True,
              "logit_count": actual.numel(), "logits_dtype": str(output.logits.dtype),
              "rmse_vs_bf16": delta.square().mean().sqrt().item(), "max_abs_error_vs_bf16": delta.abs().max().item(),
              "cosine_vs_bf16": cosine, "top10_ids": indices.tolist(), "top10_logits": values.tolist(),
              "top10_text": [tokenizer.decode([i]) for i in indices.tolist()],
              "cache_type": type(cache).__name__, "cache_layers": cache_layers, "cache_tokens": cache_tokens,
              "generation_executed": False})
        del output, actual, delta, cache

    forward("bf16_baseline_forward")
    converted = 0

    def quantize_body(parent):
        nonlocal converted
        for name in tuple(parent._modules):
            module = parent._modules[name]
            if type(module) is torch.nn.Linear:
                module.float()
                module.qconfig = torch.ao.quantization.default_dynamic_qconfig
                quantized = torch.ao.nn.quantized.dynamic.Linear.from_float(module)
                parent._modules[name] = quantized
                converted += 1
                del module, quantized
            else:
                quantize_body(module)

    started = time.monotonic()
    quantize_body(model.model)  # Excludes top-level tied lm_head.
    model.float().eval()  # FP32 embedding/norm/head, body linears already quantized.
    assert converted == 196 and model.lm_head.weight is model.model.embed_tokens.weight
    emit({"phase": "body_quantized", "linears": converted, "seconds": time.monotonic() - started})
    forward("int8_body_fp32_head_forward")
    old_head = model.lm_head  # Its parameter is still the required embedding: no weight clone.
    old_head.qconfig = torch.ao.quantization.default_dynamic_qconfig
    model.lm_head = torch.ao.nn.quantized.dynamic.Linear.from_float(old_head)
    emit({"phase": "per_tensor_head", "qscheme": str(model.lm_head.weight().qscheme()),
          "scale": model.lm_head.weight().q_scale()})
    forward("int8_body_per_tensor_head_forward")
    # Release packed head before replacing; preserve only the shared baseline head reference.
    del model.lm_head
    old_head.qconfig = torch.ao.quantization.per_channel_dynamic_qconfig
    model.lm_head = torch.ao.nn.quantized.dynamic.Linear.from_float(old_head)
    scales = model.lm_head.weight().q_per_channel_scales()
    emit({"phase": "per_channel_head", "qscheme": str(model.lm_head.weight().qscheme()),
          "scale_min": scales.min().item(), "scale_max": scales.max().item(), "scale_mean": scales.mean().item()})
    forward("int8_body_per_channel_head_forward")
    del old_head, scales
    for name, entry in manifest["files"].items():
        assert sha(MODEL / name) == entry["sha256"]
    emit({"phase": "finished", "resources_unchanged": True, "forward_count": 4,
          "generation_executed": False, "editorial_accepted": False})


def supervise():
    # Reuse only ctypes layouts of the preserved supervisor; no native imports/ML.
    from run_editorial_host_owned import ExtendedLimits
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
    limits = ExtendedLimits()
    limits.BasicLimitInformation.LimitFlags = 0x2000 | 0x200
    limits.JobMemoryLimit = CAP
    process = None
    peak = 0
    start = time.monotonic()
    report = {"cap_bytes": CAP, "deadline_seconds": 180, "accepted": False, "editorial_accepted": False,
              "generation_executed": False, "script_sha256": sha(Path(__file__)),
              "supervisor_layout_sha256": sha(ROOT / "tests/scripts/run_editorial_host_owned.py")}
    try:
        if not kernel.SetInformationJobObject(handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            raise ctypes.WinError(ctypes.get_last_error())
        env = {k: v for k, v in os.environ.items() if k.upper() in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP", "PATH")}
        env.update(HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", OMP_NUM_THREADS="2",
                   MKL_NUM_THREADS="2", TOKENIZERS_PARALLELISM="false", PYTHONDONTWRITEBYTECODE="1")
        with (output / "stdout.jsonl").open("wb") as stdout, (output / "stderr.log").open("wb") as stderr:
            process = subprocess.Popen([sys.executable, "-I", "-B", str(Path(__file__).resolve()), "--child"],
                                       stdin=subprocess.PIPE, stdout=stdout, stderr=stderr, env=env,
                                       creationflags=subprocess.CREATE_NO_WINDOW)
            if not kernel.AssignProcessToJobObject(handle, wintypes.HANDLE(int(process._handle))):
                process.terminate()
                process.wait(timeout=5)
                raise ctypes.WinError(ctypes.get_last_error())
            report["pid"] = process.pid
            process.stdin.write(b"owned-job-ready\n")
            process.stdin.close()
            while process.poll() is None:
                sample = ExtendedLimits()
                if not kernel.QueryInformationJobObject(handle, 9, ctypes.byref(sample), ctypes.sizeof(sample), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                peak = max(peak, sample.PeakJobMemoryUsed)
                if time.monotonic() - start > 180:
                    raise TimeoutError("Own diagnostic deadline reached")
                time.sleep(0.1)
            sample = ExtendedLimits()
            if kernel.QueryInformationJobObject(handle, 9, ctypes.byref(sample), ctypes.sizeof(sample), None):
                peak = max(peak, sample.PeakJobMemoryUsed)
            report["exit_code"] = process.returncode
            if process.returncode:
                raise RuntimeError("Forward diagnostic failed; preserve phase evidence")
            report["accepted"] = True
    except Exception as error:
        report["error"] = type(error).__name__ + ": " + str(error)
        raise
    finally:
        if process is not None and process.poll() is None:
            kernel.TerminateJobObject(handle, 124)
            process.wait(timeout=5)
        kernel.CloseHandle(handle)
        report.update(peak_job_memory=peak, duration_seconds=time.monotonic() - start,
                      own_process_signaled=process is not None and process.poll() is not None)
        (output / "supervisor.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        emit(report)


if __name__ == "__main__":
    if sys.argv[1] == "--child":
        child()
    else:
        sys.path.insert(0, str(Path(__file__).parent))  # Own stdlib-only layout helper, parent only.
        supervise()
