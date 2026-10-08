"""Meta-only Qwen constructor and its single CPU rotary buffer, no weights/forward."""
import gc
import hashlib
import json
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
MODEL = ROOT / ".local/models/qwen2.5-1.5b-instruct"


def emit(value):
    print(json.dumps(value, sort_keys=True), flush=True)


def child():
    if sys.stdin.buffer.readline() != b"owned-job-ready\n":
        raise ValueError("Missing supervisor handshake")
    import torch
    import psutil
    from accelerate import init_empty_weights
    from transformers import AutoConfig, AutoModelForCausalLM
    from transformers.models.qwen2.modeling_qwen2 import Qwen2RotaryEmbedding
    torch.set_num_threads(2)
    torch.set_num_interop_threads(1)
    process = psutil.Process()

    def memory(phase):
        info = process.memory_info()
        emit({"phase": phase, "private": info.private, "rss": info.rss, "vms": info.vms,
              "peak_pagefile": info.peak_pagefile})

    config_path = MODEL / "config.json"
    digest = hashlib.sha256(config_path.read_bytes()).hexdigest()
    config = AutoConfig.from_pretrained(MODEL, local_files_only=True, trust_remote_code=False)
    gc.collect()
    memory("before_true_constructor")
    start = time.monotonic()
    with init_empty_weights(include_buffers=True):
        model = AutoModelForCausalLM.from_config(config, torch_dtype=torch.float32,
                                                attn_implementation="eager")
    memory("after_true_constructor_before_gc")
    gc.collect()
    memory("after_true_constructor_after_gc")
    assert all(p.device.type == "meta" for p in model.parameters())
    buffers = dict(model.named_buffers())
    assert set(buffers) == {"model.rotary_emb.inv_freq"}
    assert buffers["model.rotary_emb.inv_freq"].device.type == "meta"
    model.model.rotary_emb = Qwen2RotaryEmbedding(config, device="cpu")
    buffers = dict(model.named_buffers())
    assert set(buffers) == {"model.rotary_emb.inv_freq"}
    frequency = buffers["model.rotary_emb.inv_freq"]
    assert frequency.device.type == "cpu" and frequency.dtype == torch.float32 and frequency.numel() == 64
    assert torch.isfinite(frequency).all()
    assert model.model.rotary_emb.original_inv_freq is frequency
    assert all(p.device.type == "meta" for p in model.parameters())
    emit({"phase": "validated_true_constructor", "seconds": time.monotonic() - start,
          "all_parameters_meta": True, "cpu_parameter_bytes": 0,
          "cpu_buffer": "model.rotary_emb.inv_freq", "cpu_buffer_bytes": frequency.numel() * frequency.element_size(),
          "original_inv_freq_alias": True, "config_sha256": digest,
          "weights_loaded": False, "forward_count": 0, "generated_tokens": 0})
    memory("after_cpu_buffer_reconstruction")
    del model, frequency, buffers
    gc.collect()
    memory("after_model_delete")
    assert hashlib.sha256(config_path.read_bytes()).hexdigest() == digest


if __name__ == "__main__":
    if sys.argv[1] == "--child":
        child()
    else:
        sys.path.insert(0, str(Path(__file__).parent))
        import test_e5_editorial_int8_micro as supervisor  # Preserved 1GiB/60s stdlib-only owner.
        supervisor.__file__ = __file__
        supervisor.supervise()
