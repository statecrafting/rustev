#!/usr/bin/env python3
"""Offline stage-2 reference. Run only after stage 1 passes.

Usage: reference.py ABSOLUTE_MODEL_DIRECTORY OUTPUT_JSON
The output contains synthetic texts and reference vectors, never model bytes.
"""
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import re
import sys


def main():
    model_dir = Path(sys.argv[1])
    output = Path(sys.argv[2])
    assert model_dir.is_absolute() and model_dir.is_dir()
    root = Path(__file__).resolve().parents[3]
    cache = root / "target" / "feasibility-011-cache"
    for key in ("HF_HOME", "TORCH_HOME", "XDG_CACHE_HOME"):
        os.environ[key] = str(cache / key.lower())
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    import numpy as np
    import onnx
    import onnxruntime as ort
    import torch
    from sentence_transformers import SentenceTransformer

    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    model = SentenceTransformer(
        str(model_dir), device="cpu", local_files_only=True,
        model_kwargs={"use_safetensors": False},
    )
    assert model.max_seq_length == 256
    source = root / "packages/rustev-pkg-support-routing/tests/common/mod.rs"
    texts = re.findall(r'case\(\s*"sr-[^"]+",\s*"(SYNTHETIC:[^"\n]*)"', source.read_text())
    assert len(texts) == 16
    texts += [" \t\n", "SYNTHETIC: café naïve 中文 日本語 😀", "!!!???...;;;",
              " ".join(["hello"] * 254), " ".join(["hello"] * 255)]
    graph_bytes = (model_dir / "onnx/model.onnx").read_bytes()
    graph = onnx.load_model_from_string(graph_bytes)
    assert not any(t.external_data for t in graph.graph.initializer)
    options = ort.SessionOptions()
    options.intra_op_num_threads = options.inter_op_num_threads = 1
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    session = ort.InferenceSession(graph_bytes, options, providers=["CPUExecutionProvider"])
    fixtures = []
    for index, text in enumerate(texts):
        raw = model.tokenizer(text, add_special_tokens=True, truncation=False)["input_ids"]
        item = {"text": text, "token_ids": raw}
        if not text.strip():
            item["refusal"] = "text_empty"
        elif len(raw) > 256:
            item["refusal"] = "too_many_tokens"
        else:
            if index == len(texts) - 2:
                assert len(raw) == 256
            features = model.tokenize([text])
            with torch.no_grad():
                reference = model(features)["sentence_embedding"][0].numpy()
            inputs = {v.name: features[v.name].numpy().astype(np.int64)
                      for v in session.get_inputs()}
            hidden = session.run(None, inputs)[0][0]
            # Match spec 3.9: f32 accumulation in token order.
            pooled = np.zeros(384, dtype=np.float32)
            for vector, mask in zip(hidden, inputs["attention_mask"][0]):
                pooled += vector * np.float32(mask)
            pooled /= np.float32(inputs["attention_mask"].sum())
            norm = np.sqrt(np.sum(pooled * pooled, dtype=np.float32))
            pooled /= norm
            delta = np.abs(pooled - reference)
            bound = np.maximum(1e-5, 1e-4 * np.maximum(np.abs(pooled), np.abs(reference)))
            item.update(reference_embedding=reference.tolist(), onnx_embedding=pooled.tolist(),
                        max_abs_difference=str(float(delta.max())),
                        cosine=str(float(np.dot(pooled, reference) /
                                         (np.linalg.norm(pooled) * np.linalg.norm(reference)))),
                        agrees=bool(np.isfinite(pooled).all() and np.isfinite(reference).all()
                                    and (delta <= bound).all()))
        fixtures.append(item)
    assert len(fixtures[-1]["token_ids"]) == 257

    def signature(values):
        return [{"name": v.name,
                 "element_type": onnx.TensorProto.DataType.Name(v.type.tensor_type.elem_type),
                 "shape": [d.dim_param if d.dim_param else d.dim_value
                           for d in v.type.tensor_type.shape.dim]} for v in values]

    result = {
        "graph_sha256": hashlib.sha256(graph_bytes).hexdigest(),
        "tokenizer_sha256": hashlib.sha256((model_dir / "tokenizer.json").read_bytes()).hexdigest(),
        "weights_sha256": hashlib.sha256((model_dir / "pytorch_model.bin").read_bytes()).hexdigest(),
        "inputs": signature(graph.graph.input), "outputs": signature(graph.graph.output),
        "operators": [{"domain": opset.domain, "opset": opset.version,
                       "operators": sorted({n.op_type for n in graph.graph.node
                                            if n.domain == opset.domain})}
                      for opset in graph.opset_import],
        "packages": {p: importlib.metadata.version(p) for p in
                     ("sentence-transformers", "torch", "onnxruntime", "onnx", "transformers", "tokenizers")},
        "settings": {"provider": "cpu", "intra_op_threads": 1, "inter_op_threads": 1,
                     "graph_optimization": "all", "precision": "f32",
                     "pooling": "mean_masked", "normalize": "l2", "max_tokens": 256},
        "fixture_source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "fixtures": fixtures,
        "verdict": "pass" if all(f.get("agrees", True) for f in fixtures) else "refuse",
    }
    output.write_text(json.dumps(result, ensure_ascii=True, indent=2) + "\n")


if __name__ == "__main__":
    main()
