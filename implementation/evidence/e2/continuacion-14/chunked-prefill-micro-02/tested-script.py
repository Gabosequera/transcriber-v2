"""Tiny random Qwen2 CPU micro: chunked prefill/cache API, not editorial quality.

No checkpoint, tokenizer, from_pretrained, downloads, worker edits or GUI.
Only the owned child imports Torch, after a 1 GiB/60 s JobObject handshake.
"""
import argparse
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
CAP = 1024 ** 3
DEADLINE = 60


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def emit(value):
    print(json.dumps(value, sort_keys=True, allow_nan=False), flush=True)


def require(ok, message):
    if not ok:
        raise AssertionError(message)


def child(output_dir, expected_worker_sha):
    if sys.stdin.buffer.readline() != b"owned-job-ready\n":
        raise ValueError("Missing owned supervisor handshake")
    require(os.environ.get("OMP_NUM_THREADS") == "2" and os.environ.get("MKL_NUM_THREADS") == "2", "Native thread limits must precede imports")
    import importlib.util
    worker_path = ROOT / "workers/python/editorial_worker.py"
    worker_source = worker_path.read_bytes()
    require(hashlib.sha256(worker_source).hexdigest() == expected_worker_sha, "Worker changed before micro")
    spec = importlib.util.spec_from_file_location("owned_editorial_prefill_micro", worker_path)
    worker = importlib.util.module_from_spec(spec)
    # Execute exact approved source bytes, never an unhashed .pyc via SourceFileLoader.
    exec(compile(worker_source, str(worker_path), "exec"), worker.__dict__)
    require("torch" not in sys.modules, "Worker import must remain stdlib-only")
    import importlib.metadata
    versions = {name: importlib.metadata.version(name) for name in ("torch", "transformers")}
    require(versions == {"torch": "2.8.0+cpu", "transformers": "4.48.3"}, "Pinned micro runtime differs")
    import torch
    from transformers import Qwen2Config, Qwen2ForCausalLM, LogitsProcessor, LogitsProcessorList, RepetitionPenaltyLogitsProcessor
    from transformers.cache_utils import DynamicCache
    torch.set_num_threads(2)
    torch.set_num_interop_threads(1)
    torch.manual_seed(421)
    started = time.monotonic()
    def guard():
        require(time.monotonic()-started < 50, "Micro inner deadline")

    class GeneratedOnlyPenalty(LogitsProcessor):
        def __init__(self, prompt_len, penalty=1.1):
            self.prompt_len = prompt_len
            self.delegate = RepetitionPenaltyLogitsProcessor(penalty)

        def __call__(self, ids, scores):
            generated = ids[:, self.prompt_len:]
            if generated.shape[1] == 0:
                return scores
            return self.delegate(generated, scores)

    # Explicit simple scores prove that prompt-only tokens are not penalized.
    prompt = torch.tensor([[5, 7, 9]], dtype=torch.long)
    score = torch.tensor([[float(index-8) for index in range(16)]], dtype=torch.float32)
    processor = GeneratedOnlyPenalty(3)
    unchanged = processor(prompt, score.clone())
    require(torch.equal(unchanged, score), "Prompt with no outputs must not change scores")
    penalized = processor(torch.tensor([[5, 7, 9, 7, 11]], dtype=torch.long), score.clone())
    require(penalized[0, 5].item() == score[0, 5].item(), "Prompt-only token was penalized")
    require(torch.isclose(penalized[0, 7], score[0, 7]*1.1).item(), "Generated negative score penalty incorrect")
    require(torch.isclose(penalized[0, 11], score[0, 11]/1.1).item(), "Generated positive score penalty incorrect")
    emit({"phase": "generated_only_penalty", "prefix_without_outputs_unchanged": True,
          "prompt_only_token_unpenalized": True, "generated_tokens_penalized": True, "penalty": 1.1})

    config = Qwen2Config(vocab_size=128, hidden_size=64, intermediate_size=128, num_hidden_layers=2,
                        num_attention_heads=4, num_key_value_heads=2, max_position_embeddings=1024,
                        rope_theta=1000000.0, tie_word_embeddings=True, attention_dropout=0.0,
                        bos_token_id=1, eos_token_id=None, pad_token_id=0, use_cache=True,
                        attn_implementation="sdpa")
    model = Qwen2ForCausalLM(config).requires_grad_(False).eval()
    require(model.config._attn_implementation == "sdpa", "SDPA required")
    ids = (((torch.arange(513, dtype=torch.long)*7+3) % 126)+1)[None, :]
    mask = torch.ones_like(ids)
    original_ids, original_mask = ids.clone(), mask.clone()
    with torch.inference_mode():
        full = model(input_ids=ids, attention_mask=mask, past_key_values=DynamicCache(), use_cache=True, num_logits_to_keep=0)
        reference = full.logits[:, -1, :].clone()
        require(bool(torch.isfinite(full.logits).all()), "Full logits nonfinite")
        cache = DynamicCache()
        chunks = []
        for start in range(0, 513, 256):
            guard()
            end = min(start+256, 513)
            segmented = model(input_ids=ids[:, start:end], attention_mask=mask[:, :end],
                              past_key_values=cache, cache_position=torch.arange(start, end), use_cache=True, num_logits_to_keep=1)
            cache = segmented.past_key_values
            require(bool(torch.isfinite(segmented.logits).all()), "Chunk logits nonfinite")
            chunks.append([start, end, cache.get_seq_length()])
        last_chunk = segmented.logits[:, -1, :]
        max_logit_error = (reference-last_chunk).abs().max().item()
        torch.testing.assert_close(last_chunk, reference, atol=1e-5, rtol=1e-5)
        kv_error = 0.0
        for layer in range(2):
            for kind in ("key_cache", "value_cache"):
                actual, expected = getattr(cache, kind)[layer], getattr(full.past_key_values, kind)[layer]
                kv_error = max(kv_error, (actual-expected).abs().max().item())
                torch.testing.assert_close(actual, expected, atol=1e-5, rtol=1e-5)
        del cache, segmented, last_chunk
        progress = []
        prefix = worker.prefill_prefix(model, {"input_ids": ids, "attention_mask": mask}, guard, progress.append)
        prefix_length = prefix.get_seq_length()
        require(prefix_length == 512, "Prefix cache must contain exactly N-1 tokens")
        require(progress == [.5, 1.0], "Production chunks/progress must cover both 256-token prefixes")
        for layer in range(2):
            for kind in ("key_cache", "value_cache"):
                expected = getattr(full.past_key_values, kind)[layer][:, :, :512, :]
                torch.testing.assert_close(getattr(prefix, kind)[layer], expected, atol=1e-5, rtol=1e-5)
        del full
        # Validate exactly what HF generation will consume with full IDs + N-1 cache.
        kwargs = model._get_initial_cache_position(ids, {"past_key_values": prefix, "attention_mask": mask})
        prepared = model.prepare_inputs_for_generation(ids, **kwargs)
        require(prepared["cache_position"].tolist() == [512], "HF should consume the final prompt token only")
        require(torch.equal(prepared["input_ids"], ids[:, -1:]), "HF consumed wrong prompt suffix")
        require(prepared["attention_mask"].shape[1] == 513, "HF lost full prompt mask")
        generation = {"max_new_tokens": 8, "do_sample": False, "repetition_penalty": 1.0,
                      "pad_token_id": 0, "eos_token_id": None, "use_cache": True}
        baseline = model.generate(input_ids=ids, attention_mask=mask, **generation,
                                  logits_processor=LogitsProcessorList([GeneratedOnlyPenalty(513)]))
        guard()
        chunked = model.generate(input_ids=ids, attention_mask=mask, past_key_values=prefix, **generation,
                                 logits_processor=LogitsProcessorList([GeneratedOnlyPenalty(513)]))
        require(baseline.shape == chunked.shape == (1, 521), "Full prompt plus eight outputs required")
        require(torch.equal(baseline[:, :513], original_ids) and torch.equal(chunked[:, :513], original_ids), "Returned context was cut")
        require(torch.equal(baseline, chunked), "Chunked and full generated tokens differ")
        require(torch.equal(ids, original_ids) and torch.equal(mask, original_mask), "Inputs mutated")
        # Production boundary N=1: no prefill forward, then generate complete input.
        forwards = []
        hook = model.register_forward_pre_hook(lambda module, args: forwards.append(1))
        one_ids, one_mask = ids[:, :1], mask[:, :1]
        try:
            one_progress = []
            empty = worker.prefill_prefix(model, {"input_ids": one_ids, "attention_mask": one_mask}, guard, one_progress.append)
            require(empty.get_seq_length() == 0 and not forwards and not one_progress, "N=1 must have empty cache and no forward")
            one_baseline = model.generate(input_ids=one_ids, attention_mask=one_mask, **generation,
                                          logits_processor=LogitsProcessorList([GeneratedOnlyPenalty(1)]))
            one_chunked = model.generate(input_ids=one_ids, attention_mask=one_mask, past_key_values=empty, **generation,
                                         logits_processor=LogitsProcessorList([GeneratedOnlyPenalty(1)]))
            require(one_baseline.shape == one_chunked.shape == (1, 9) and torch.equal(one_baseline, one_chunked), "N=1 generation differs")
            forwards.clear()
            def cancel_guard():
                raise worker.EditorialError("E_CANCELLED", "Synthetic cancellation before first forward")
            try:
                worker.prefill_prefix(model, {"input_ids": ids, "attention_mask": mask}, cancel_guard, lambda fraction: None)
            except worker.EditorialError as error:
                require(error.code == "E_CANCELLED", "Unexpected cancellation code")
            else:
                raise AssertionError("Production helper ignored guard cancellation")
            require(not forwards, "Cancelled guard allowed a model forward")
        finally:
            hook.remove()
    require(sha(worker_path) == expected_worker_sha, "Worker changed during micro")
    report = {"accepted": True, "model": "TinyQwen2 random weights, NOT real checkpoint", "random_seed": 421,
              "runtime": versions, "torch_threads": torch.get_num_threads(), "interop_threads": torch.get_num_interop_threads(),
              "attention": "sdpa", "parameter_count": sum(p.numel() for p in model.parameters()),
              "input_tokens": 513, "chunk_size": 256, "full_forward_chunks": chunks,
              "production_helper_exercised": "editorial_worker.prefill_prefix", "worker_sha256": expected_worker_sha,
              "worker_import_stdlib_only": True, "worker_unchanged": True, "prefill_progress": progress,
              "prefix_cache_tokens_before_generate": prefix_length, "initial_generate_cache_position": [512],
              "full_last_logits_finite": True, "chunk_logits_finite": True, "max_last_logit_abs_error": max_logit_error,
              "max_full_chunk_kv_abs_error": kv_error, "comparison_atol": 1e-5, "comparison_rtol": 1e-5,
              "new_tokens": 8, "baseline_token_ids": baseline[0, 513:].tolist(), "chunked_token_ids": chunked[0, 513:].tolist(),
              "one_token_empty_prefill": True, "one_token_generation_equal": True,
              "cancelled_before_forward": True, "cancelled_forward_count": len(forwards),
              "generated_tokens_exactly_equal": True, "context_uncut": True, "generated_only_penalty": 1.1,
              "generation_repetition_penalty": 1.0, "editorial_quality_accepted": False, "real_model_weights_loaded": False,
              "downloads": False, "worker_modified": False, "duration_seconds": time.monotonic()-started}
    Path(output_dir, "micro.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    emit(report)


def supervise(output):
    from run_editorial_host_owned import ExtendedLimits
    output = output.resolve()
    if output.exists() or not output.is_relative_to(ROOT / ".local") or output == ROOT / ".local":
        raise ValueError("Fresh owned V2 .local evidence directory required")
    output.mkdir(parents=True)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
    kernel.CreateJobObjectW.restype = wintypes.HANDLE
    kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    kernel.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
    kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    kernel.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    job = kernel.CreateJobObjectW(None, None)
    if not job:
        raise ctypes.WinError(ctypes.get_last_error())
    process = None
    peak = 0
    started = time.monotonic()
    worker_sha = sha(ROOT / "workers/python/editorial_worker.py")
    report = {"accepted": False, "cap_bytes": CAP, "deadline_seconds": DEADLINE, "worker_sha256": worker_sha,
              "script_sha256": sha(__file__), "real_model_weights_loaded": False, "editorial_quality_accepted": False}
    try:
        limits = ExtendedLimits()
        limits.BasicLimitInformation.LimitFlags = 0x2000 | 0x200
        limits.JobMemoryLimit = CAP
        if not kernel.SetInformationJobObject(job, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            raise ctypes.WinError(ctypes.get_last_error())
        env = {key: value for key, value in os.environ.items() if key.upper() in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP", "PATH")}
        env.update(OMP_NUM_THREADS="2", MKL_NUM_THREADS="2", HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1",
                   TOKENIZERS_PARALLELISM="false", PYTHONDONTWRITEBYTECODE="1")
        with (output / "stdout.jsonl").open("wb") as stdout, (output / "stderr.log").open("wb") as stderr:
            process = subprocess.Popen([sys.executable, "-I", "-B", str(Path(__file__).resolve()), "--child", str(output), worker_sha],
                                       stdin=subprocess.PIPE, stdout=stdout, stderr=stderr, env=env, creationflags=subprocess.CREATE_NO_WINDOW)
            if not kernel.AssignProcessToJobObject(job, wintypes.HANDLE(int(process._handle))):
                process.terminate()
                process.wait(timeout=5)
                raise ctypes.WinError(ctypes.get_last_error())
            report["pid"] = process.pid
            process.stdin.write(b"owned-job-ready\n")
            process.stdin.close()
            report["supervisor_handshake"] = True
            while process.poll() is None:
                sample = ExtendedLimits()
                if not kernel.QueryInformationJobObject(job, 9, ctypes.byref(sample), ctypes.sizeof(sample), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                peak = max(peak, sample.PeakJobMemoryUsed)
                if time.monotonic()-started > DEADLINE:
                    raise TimeoutError("Owned tiny micro deadline")
                time.sleep(.05)
            sample = ExtendedLimits()
            if kernel.QueryInformationJobObject(job, 9, ctypes.byref(sample), ctypes.sizeof(sample), None):
                peak = max(peak, sample.PeakJobMemoryUsed)
            report["exit_code"] = process.returncode
            require(process.returncode == 0, "Tiny micro failed; preserve evidence")
            require(json.loads((output / "micro.json").read_text(encoding="utf-8"))["accepted"], "Missing micro result")
            report["accepted"] = True
    except Exception as error:
        report["error"] = type(error).__name__ + ": " + str(error)
        raise
    finally:
        if process is not None and process.poll() is None:
            kernel.TerminateJobObject(job, 124)
            process.wait(timeout=5)
        kernel.CloseHandle(job)
        report.update(peak_job_memory=peak, duration_seconds=time.monotonic()-started,
                      own_process_signaled=process is not None and process.poll() is not None)
        (output / "supervisor.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        emit(report)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        child(sys.argv[2], sys.argv[3])
    else:
        parser = argparse.ArgumentParser()
        parser.add_argument("output", type=Path)
        arguments = parser.parse_args()
        supervise(arguments.output)
