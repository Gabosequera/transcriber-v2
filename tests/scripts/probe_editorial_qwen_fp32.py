"""Direct FP32/meta diagnostic: two forwards and at most 16 short-prompt tokens."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
MODEL = ROOT / ".local/models/qwen2.5-1.5b-instruct"
PREVIOUS = ROOT / ".local/e5-editorial-qwen-forward-01/stdout.jsonl"
JOB = ROOT / ".local/e5-editorial-host-04/run/jobs/job-d2933e2362a4.json"
INPUTS = ROOT / ".local/e5-editorial-host-04/run/job-d2933e2362a4/inputs"


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for part in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(part)
    return digest.hexdigest()


def emit(value):
    print(json.dumps(value, ensure_ascii=False, sort_keys=True, allow_nan=False), flush=True)


def child():
    if sys.stdin.buffer.readline() != b"owned-job-ready\n":
        raise ValueError("Missing supervisor handshake")
    manifest = json.loads((MODEL / "model-manifest.json").read_text(encoding="utf-8"))
    for name, entry in manifest["files"].items():
        assert (MODEL / name).stat().st_size == entry["size"] and sha(MODEL / name) == entry["sha256"]
    record = json.loads(JOB.read_text(encoding="utf-8"))
    worker_path = ROOT / "workers/python/editorial_worker.py"
    worker_sha = sha(worker_path)
    assert worker_sha == record["payload"]["backend"]["worker_sha256"]
    documents = {}
    input_hashes = {}
    for name, text in record["payload"]["input"]["documents"].items():
        path = INPUTS / name
        assert path.resolve().is_relative_to(INPUTS.resolve())
        expected = hashlib.sha256(text.encode()).hexdigest()
        assert sha(path) == expected
        input_hashes[name] = expected
        if name.endswith(".json"):
            documents[name] = json.loads(text)
    spec = importlib.util.spec_from_file_location("owned_editorial_prompt", worker_path)
    worker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(worker)  # Stdlib only; no worker initialization/run/native import.
    import importlib.metadata as metadata
    lock = ROOT / "workers/python/requirements-editorial.lock"
    dependencies = dict(line.split("==") for line in lock.read_text(encoding="utf-8").splitlines() if line.strip())
    assert all(metadata.version(name) == version for name, version in dependencies.items())
    emit({"phase": "resources_verified", "worker_sha256": worker_sha,
          "model_manifest_sha256": sha(MODEL / "model-manifest.json"), "lock_sha256": sha(lock),
          "helper_sha256": sha(ROOT / "tests/scripts/probe_editorial_qwen_quantization.py"),
          "input_hashes": input_hashes, "dependencies": dependencies})
    import torch
    from transformers import AutoModelForCausalLM, AutoTokenizer
    torch.set_num_threads(2)
    torch.set_num_interop_threads(1)
    torch.manual_seed(0)
    tokenizer = AutoTokenizer.from_pretrained(MODEL, local_files_only=True, trust_remote_code=False)
    short_messages = [{"role": "system", "content": "JSON only."},
                      {"role": "user", "content": 'Devuelve {"tema":"saludo"}.'}]
    short = tokenizer.apply_chat_template(short_messages, tokenize=True, add_generation_prompt=True, return_tensors="pt")
    assert short.shape[1] <= 32
    actual_messages = worker.messages_for(documents, record["payload"]["input"]["kind"],
                                         record["payload"]["input"]["parameters"])
    actual_text = tokenizer.apply_chat_template(actual_messages, tokenize=False, add_generation_prompt=True)
    actual = tokenizer(actual_text, return_tensors="pt")["input_ids"]
    assert actual.shape[1] <= 4096
    emit({"phase": "prompts", "short_tokens": short.shape[1], "actual_context_tokens": actual.shape[1],
          "actual_context_utf8_bytes": len(actual_text.encode()), "short_input_ids": short.tolist()[0],
          "actual_context_sha256": hashlib.sha256(actual_text.encode()).hexdigest()})
    started = time.monotonic()
    emit({"phase": "load_direct_fp32"})
    model = AutoModelForCausalLM.from_pretrained(MODEL, local_files_only=True, trust_remote_code=False,
                                               use_safetensors=True, torch_dtype=torch.float32,
                                               attn_implementation="eager", low_cpu_mem_usage=True).eval()
    emit({"phase": "loaded_direct_fp32", "seconds": time.monotonic() - started,
          "tied_head_embedding": model.lm_head.weight is model.model.embed_tokens.weight})
    previous = [json.loads(line) for line in PREVIOUS.read_text(encoding="utf-8").splitlines()]
    bf16 = next(row for row in previous if row["phase"] == "bf16_baseline_forward")

    def forward(ids, phase):
        started = time.monotonic()
        with torch.inference_mode():
            output = model(input_ids=ids, attention_mask=torch.ones_like(ids),
                           use_cache=True, num_logits_to_keep=1)
        logits = output.logits[0, -1].float()
        assert torch.isfinite(logits).all()
        values, indices = logits.topk(10)
        cache = output.past_key_values
        result = {"phase": phase, "seconds": time.monotonic() - started, "finite": True,
                  "input_tokens": ids.shape[1], "logits_dtype": str(output.logits.dtype),
                  "top10_ids": indices.tolist(), "top10_logits": values.tolist(),
                  "top10_text": [tokenizer.decode([i]) for i in indices.tolist()],
                  "cache_type": type(cache).__name__, "cache_layers": len(cache),
                  "cache_tokens": cache.get_seq_length() if hasattr(cache, "get_seq_length") else cache[0][0].shape[-2]}
        if phase == "short_direct_fp32_forward":
            result["previous_bf16_top10_id_logit_deltas"] = [logits[i].item() - v for i, v in zip(bf16["top10_ids"], bf16["top10_logits"])]
        emit(result)
        del output, logits, cache

    forward(short, "short_direct_fp32_forward")
    forward(actual, "actual_context_direct_fp32_forward")
    started = time.monotonic()
    with torch.inference_mode():
        output = model.generate(input_ids=short, attention_mask=torch.ones_like(short),
                                max_new_tokens=16, do_sample=False, use_cache=True,
                                num_logits_to_keep=1, pad_token_id=tokenizer.eos_token_id)
    tokens = output[0, short.shape[1]:]
    emit({"phase": "short_generate16", "seconds": time.monotonic() - started,
          "generated_tokens": len(tokens), "token_ids": tokens.tolist(),
          "raw": tokenizer.decode(tokens, skip_special_tokens=True), "editorial_accepted": False})
    del output, tokens
    for name, entry in manifest["files"].items():
        assert sha(MODEL / name) == entry["sha256"]
    assert sha(worker_path) == worker_sha
    for name, expected in input_hashes.items():
        assert sha(INPUTS / name) == expected
    emit({"phase": "finished", "resources_unchanged": True, "forward_count": 2,
          "short_generation_limit": 16, "editorial_accepted": False})


if __name__ == "__main__":
    if sys.argv[1] == "--child":
        child()
    else:
        # Same preserved stdlib-only supervisor: 8GiB,180s,own-process kill, no retry.
        sys.path.insert(0, str(Path(__file__).parent))
        import probe_editorial_qwen_quantization as supervisor
        supervisor.__file__ = __file__  # Its child launch and script receipt refer to this new script.
        supervisor.supervise()
