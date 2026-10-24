//! BPM detection from the audio (#536).
//!
//! The beats come from "Beat This!", a neural beat tracker, run on the pure-Rust
//! `rten` runtime with its small model embedded in the binary (see
//! `models/README.md`), so there is no native library to build on any target and
//! no file to find at run time. The tempo is then fitted through the beats
//! ([`fitted_bpm`]) rather than read off the median gap between them, which the
//! tracker's 20 ms frames quantise: on 120 real files the median gave a tempo
//! within 1 BPM of the reference 34% of the time, the fit 82%.
//!
//! The audio is decoded through the same path playback and the waveform use, so
//! a file the player can play is a file whose tempo can be measured.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use beat_this::{BeatThis, Model, Tensor};
use rodio::Source;
use rten::{NodeId, ValueView};
use rten_tensor::Layout;

static MEL_MODEL: &[u8] = include_bytes!("../models/mel_spectrogram.onnx");
static BEAT_MODEL: &[u8] = include_bytes!("../models/beat_this_small.onnx");

/// Mono frames handed to the tracker at a time, about 1.5 s of 44.1 kHz audio.
/// The whole file never has to sit in memory, and a cancel is looked at between
/// chunks.
const CHUNK_FRAMES: usize = 65_536;

/// Fewer beats than this in one steady run is not a tempo anyone could check.
const MIN_BEATS: usize = 8;

/// How far a gap between two beats may stray from the median gap and still
/// belong to the steady run the tempo is fitted through.
const STEADY_TOLERANCE: f64 = 0.15;

/// Why a file has no tempo to report.
#[derive(Debug)]
pub enum TempoError {
    /// The cancel flag was raised while this file was being analysed.
    Cancelled,
    /// The file could not be decoded or analysed.
    Failed(String),
}

/// The beat tracker, loaded once and reused for every file of a run.
pub struct TempoDetector {
    tracker: BeatThis<OnnxModel>,
}

impl TempoDetector {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            tracker: BeatThis::from_models(
                OnnxModel::load(MEL_MODEL)?,
                OnnxModel::load(BEAT_MODEL)?,
            ),
        })
    }

    /// The tempo of `path` in beats per minute, unrounded and as the beats fall:
    /// folding it into a range is the caller's business. `None` when there is no
    /// steady beat to fit (a beatless intro track, spoken word).
    ///
    /// Blocking and slow, seconds per track. The caller must keep it off the
    /// main thread.
    pub fn detect(&mut self, path: &Path, cancel: &AtomicBool) -> Result<Option<f64>, TempoError> {
        let decoder = crate::open_decoder(path).map_err(TempoError::Failed)?;
        let channels = decoder.channels().get() as usize;
        let rate = decoder.sample_rate().get();
        self.detect_samples(decoder, channels, rate, cancel)
    }

    /// [`detect`](Self::detect) on interleaved samples.
    fn detect_samples(
        &mut self,
        samples: impl Iterator<Item = f32>,
        channels: usize,
        rate: u32,
        cancel: &AtomicBool,
    ) -> Result<Option<f64>, TempoError> {
        let failed = |err: anyhow::Error| TempoError::Failed(err.to_string());
        let mut stream = self.tracker.stream(rate).map_err(failed)?;
        let mut mono = Vec::with_capacity(CHUNK_FRAMES);
        let (mut frame_sum, mut in_frame) = (0.0f32, 0usize);
        for sample in samples {
            frame_sum += sample;
            in_frame += 1;
            if in_frame < channels {
                continue;
            }
            mono.push(frame_sum / channels as f32);
            (frame_sum, in_frame) = (0.0, 0);
            if mono.len() == CHUNK_FRAMES {
                if cancel.load(Ordering::Relaxed) {
                    return Err(TempoError::Cancelled);
                }
                stream.push(&mono).map_err(failed)?;
                mono.clear();
            }
        }
        if !mono.is_empty() {
            stream.push(&mono).map_err(failed)?;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(TempoError::Cancelled);
        }
        let analysis = stream.finish().map_err(failed)?;
        Ok(fitted_bpm(&analysis.beats))
    }
}

/// The tempo through `beats` (times in seconds): a least-squares line of beat
/// number against beat time over the longest run of evenly spaced beats, so a
/// breakdown or a tempo change at the edges does not pull it, and every beat in
/// the run counts rather than one gap between two of them.
pub fn fitted_bpm(beats: &[f32]) -> Option<f64> {
    if beats.len() < MIN_BEATS {
        return None;
    }
    let gaps: Vec<f64> = beats.windows(2).map(|w| f64::from(w[1] - w[0])).collect();
    let mut sorted = gaps.clone();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    // Gap i lies between beats i and i + 1, so a run of steady gaps from `start`
    // to `end` (exclusive) covers beats `start..=end`.
    let (mut best, mut start) = ((0, 0), 0);
    for index in 0..=gaps.len() {
        let steady = gaps
            .get(index)
            .is_some_and(|gap| (gap - median).abs() <= median * STEADY_TOLERANCE);
        if steady {
            continue;
        }
        if index - start > best.1 - best.0 {
            best = (start, index);
        }
        start = index + 1;
    }
    let run = &beats[best.0..=best.1];
    if run.len() < MIN_BEATS {
        return None;
    }
    let count = run.len() as f64;
    let mean_index = (count - 1.0) / 2.0;
    let mean_time = run.iter().map(|&t| f64::from(t)).sum::<f64>() / count;
    let (mut covariance, mut variance) = (0.0, 0.0);
    for (index, &time) in run.iter().enumerate() {
        let offset = index as f64 - mean_index;
        covariance += offset * (f64::from(time) - mean_time);
        variance += offset * offset;
    }
    let period = covariance / variance;
    (period > 0.0).then(|| 60.0 / period)
}

/// An ONNX graph on `rten`, loaded from bytes in the binary. The tracker's own
/// `rten` runtime only loads from a path; this is the same adapter over
/// [`rten::Model::load_static_slice`].
struct OnnxModel {
    graph: rten::Model,
    inputs: HashMap<String, NodeId>,
    outputs: Vec<(NodeId, String)>,
}

impl OnnxModel {
    fn load(bytes: &'static [u8]) -> Result<Self, String> {
        let graph = rten::Model::load_static_slice(bytes).map_err(|err| err.to_string())?;
        let named = |ids: &[NodeId]| -> Vec<(NodeId, String)> {
            ids.iter()
                .filter_map(|&id| Some((id, graph.node_info(id)?.name()?.to_string())))
                .collect()
        };
        let inputs = named(graph.input_ids())
            .into_iter()
            .map(|(id, name)| (name, id))
            .collect();
        let outputs = named(graph.output_ids());
        Ok(Self {
            graph,
            inputs,
            outputs,
        })
    }
}

impl Model for OnnxModel {
    fn run(&mut self, inputs: &[(&str, &Tensor)]) -> anyhow::Result<HashMap<String, Tensor>> {
        let mut values = Vec::with_capacity(inputs.len());
        for (name, tensor) in inputs {
            let id = *self
                .inputs
                .get(*name)
                .ok_or_else(|| anyhow::anyhow!("the model has no input '{name}'"))?;
            let view = ValueView::from_shape(tensor.shape.as_slice(), tensor.data.as_slice())
                .map_err(|err| anyhow::anyhow!("input '{name}': {err}"))?;
            values.push((id, view.into()));
        }
        let ids: Vec<NodeId> = self.outputs.iter().map(|(id, _)| *id).collect();
        let results = self.graph.run(values, &ids, None)?;
        let mut named = HashMap::new();
        for ((_, name), value) in self.outputs.iter().zip(results) {
            let tensor = value
                .into_tensor::<f32>()
                .ok_or_else(|| anyhow::anyhow!("output '{name}' is not f32"))?;
            let shape = tensor.shape().to_vec();
            named.insert(
                name.clone(),
                Tensor {
                    shape,
                    data: tensor.into_data(),
                },
            );
        }
        Ok(named)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Beats `period` seconds apart from `start`, each nudged by up to ±10 ms
    /// the way a tracker's frames place them.
    fn beats(count: usize, start: f32, period: f32) -> Vec<f32> {
        (0..count)
            .map(|i| start + i as f32 * period + [0.0, 0.01, -0.01, 0.005][i % 4])
            .collect()
    }

    #[test]
    fn the_tempo_is_fitted_through_every_beat_not_one_gap() {
        // 174 BPM, which a 20 ms frame grid can only read as 171.4 or 176.5.
        let bpm = fitted_bpm(&beats(200, 0.3, 60.0 / 174.0)).unwrap();
        assert!((bpm - 174.0).abs() < 0.1, "{bpm}");
    }

    #[test]
    fn a_break_at_the_edge_does_not_pull_the_fit() {
        // A steady 128 BPM, then a stretch of half-time beats after a breakdown.
        let mut all = beats(120, 0.0, 60.0 / 128.0);
        let last = *all.last().unwrap();
        all.extend((1..=20).map(|i| last + i as f32 * 2.0 * 60.0 / 128.0 + 0.3));
        let bpm = fitted_bpm(&all).unwrap();
        assert!((bpm - 128.0).abs() < 0.1, "{bpm}");
    }

    #[test]
    fn too_few_steady_beats_is_no_tempo() {
        assert_eq!(fitted_bpm(&beats(7, 0.0, 0.5)), None);
        assert_eq!(fitted_bpm(&[]), None);
        // Plenty of beats, none of them evenly spaced.
        let scattered: Vec<f32> = (0..40).map(|i| (i * i) as f32 * 0.05).collect();
        assert_eq!(fitted_bpm(&scattered), None);
    }

    /// A kick drum at `bpm` over a quiet hi-hat on the off-beats, stereo.
    fn click_track(bpm: f32, secs: f32, rate: u32) -> Vec<f32> {
        let period = 60.0 / bpm;
        let mut out = Vec::new();
        for n in 0..(secs * rate as f32) as usize {
            let t = n as f32 / rate as f32;
            let since_beat = t % period;
            let since_offbeat = (t + period / 2.0) % period;
            let kick = (-since_beat * 30.0).exp()
                * (std::f32::consts::TAU
                    * (50.0 + 100.0 * (-since_beat * 40.0).exp())
                    * since_beat)
                    .sin();
            let noise = ((n as u32).wrapping_mul(2_654_435_761) >> 16) as f32 / 65_536.0 - 0.5;
            let hat = (-since_offbeat * 200.0).exp() * noise * 0.3;
            let sample = 0.8 * kick + hat;
            out.push(sample);
            out.push(sample);
        }
        out
    }

    #[test]
    fn a_drum_loop_measures_at_its_tempo() {
        let mut detector = TempoDetector::new().unwrap();
        let samples = click_track(124.0, 30.0, 22_050);
        let bpm = detector
            .detect_samples(samples.into_iter(), 2, 22_050, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        assert!((bpm - 124.0).abs() < 1.0, "{bpm}");
    }

    #[test]
    fn a_raised_cancel_stops_the_analysis() {
        let mut detector = TempoDetector::new().unwrap();
        let samples = click_track(124.0, 10.0, 22_050);
        let result =
            detector.detect_samples(samples.into_iter(), 2, 22_050, &AtomicBool::new(true));
        assert!(matches!(result, Err(TempoError::Cancelled)));
    }
}
