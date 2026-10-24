# Beat tracking models

The two ONNX models behind BPM detection (#536), embedded into the binary by
`src/tempo.rs`:

| File | Size | SHA-256 |
|---|---|---|
| `mel_spectrogram.onnx` | 270,742 bytes | `fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9` |
| `beat_this_small.onnx` | 10,555,592 bytes | `a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f` |

Both come unchanged from the `models/` directory of
[beat-this-rs](https://github.com/danigb/beat-this-rs), the Rust port of
["Beat This!"](https://github.com/CPJKU/beat_this) by the Institute of
Computational Perception, JKU Linz. They are distributed under the MIT License;
see [LICENSE](LICENSE) for both copyright notices.

The small model rather than the full 83 MB one: on 120 real files it agreed with
the reference BPM as often (82% against 83%), in two thirds of the time.
