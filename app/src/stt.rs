//! Speech-to-text with Kyutai STT (`stt-1b-en_fr`), streaming, through the `moshi` crate.
//!
//! Model loading adapted from kyutai-labs/delayed-streams-modeling, `stt-rs`
//! (MIT / Apache-2.0, see spikes/stt-rs). Words come out about 0.5 s after being spoken and
//! are final: they are never revised, so they can be typed as they arrive.

use anyhow::{Context, Result};
use candle::{Device, Tensor};

/// The model works on 24 kHz mono audio, in steps of 80 ms.
pub const SAMPLE_RATE: u32 = 24_000;
const STEP: usize = 1920;

#[derive(Debug, serde::Deserialize)]
struct SttParams {
    audio_silence_prefix_seconds: f64,
    audio_delay_seconds: f64,
}

#[derive(Debug, serde::Deserialize)]
struct ModelConfig {
    mimi_name: String,
    tokenizer_name: String,
    card: usize,
    text_card: usize,
    dim: usize,
    n_q: usize,
    context: usize,
    max_period: f64,
    num_heads: usize,
    num_layers: usize,
    causal: bool,
    stt_config: SttParams,
}

impl ModelConfig {
    fn lm_config(&self) -> moshi::lm::Config {
        let transformer = moshi::transformer::Config {
            d_model: self.dim,
            num_heads: self.num_heads,
            num_layers: self.num_layers,
            dim_feedforward: self.dim * 4,
            causal: self.causal,
            norm_first: true,
            bias_ff: false,
            bias_attn: false,
            layer_scale: None,
            context: self.context,
            max_period: self.max_period as usize,
            use_conv_block: false,
            use_conv_bias: true,
            cross_attention: None,
            gating: Some(candle_nn::Activation::Silu),
            norm: moshi::NormType::RmsNorm,
            positional_embedding: moshi::transformer::PositionalEmbedding::Rope,
            conv_layout: false,
            conv_kernel_size: 3,
            kv_repeat: 1,
            max_seq_len: 4096 * 4,
            shared_cross_attn: false,
        };
        moshi::lm::Config {
            transformer,
            depformer: None,
            audio_vocab_size: self.card + 1,
            text_in_vocab_size: self.text_card + 1,
            text_out_vocab_size: self.text_card,
            audio_codebooks: self.n_q,
            conditioners: Default::default(),
            extra_heads: None,
        }
    }
}

pub struct SttModel {
    state: moshi::asr::State,
    tokenizer: sentencepiece::SentencePieceProcessor,
    device: Device,
    silence_prefix: usize,
    flush_suffix: usize,
    pending: Vec<f32>,
}

impl SttModel {
    /// Loads the model from the Hugging Face Hub (shared cache, downloaded once: about 2.4 GB).
    pub fn load(repo: &str, device: &str) -> Result<Self> {
        let device = select_device(device)?;
        let api = hf_hub::api::sync::Api::new().context("cannot reach the Hugging Face cache")?;
        let repo = api.model(repo.to_string());
        let config_file = repo.get("config.json").context("cannot download the STT config")?;
        let config: ModelConfig = serde_json::from_str(&std::fs::read_to_string(&config_file)?)
            .context("invalid STT config")?;
        let tokenizer_file = repo.get(&config.tokenizer_name)?;
        let model_file = repo.get("model.safetensors").context("cannot download the STT model")?;
        let mimi_file = repo.get(&config.mimi_name)?;

        let tokenizer = sentencepiece::SentencePieceProcessor::open(&tokenizer_file)
            .context("cannot load the STT tokenizer")?;
        let dtype = device.bf16_default_to_f32();
        // SAFETY: the weights file is not modified while mapped (it sits in the read-only
        // Hugging Face cache).
        let weights = unsafe { candle_nn::VarBuilder::from_mmaped_safetensors(&[&model_file], dtype, &device)? };
        let lm = moshi::lm::LmModel::new(&config.lm_config(), moshi::nn::MaybeQuantizedVarBuilder::Real(weights))?;
        let mimi_path = mimi_file.to_str().context("non UTF-8 cache path")?;
        let audio_tokenizer = moshi::mimi::load(mimi_path, Some(config.n_q), &device)?;
        let delay_tokens = (config.stt_config.audio_delay_seconds * 12.5) as usize;
        let state = moshi::asr::State::new(1, delay_tokens, 0.0, audio_tokenizer, lm)?;

        let seconds = |s: f64| (s * SAMPLE_RATE as f64) as usize;
        let mut model = Self {
            state,
            tokenizer,
            device,
            silence_prefix: seconds(config.stt_config.audio_silence_prefix_seconds),
            // The delay, plus margin, of silence pushes the last words out.
            flush_suffix: seconds(config.stt_config.audio_delay_seconds + 0.5),
            pending: Vec::new(),
        };
        // Warm-up: the first steps compile GPU kernels; better now than in the first dictation.
        model.begin()?;
        model.finish(&mut |_| {})?;
        Ok(model)
    }

    pub fn device_name(&self) -> &'static str {
        match self.device {
            Device::Cpu => "cpu",
            Device::Cuda(_) => "cuda",
            Device::Metal(_) => "metal",
        }
    }

    /// Starts a new utterance.
    pub fn begin(&mut self) -> Result<()> {
        self.state.reset()?;
        self.pending.clear();
        let prefix = vec![0.0; self.silence_prefix];
        self.feed(&prefix, &mut |_| {})
    }

    /// Feeds 24 kHz mono samples; `on_word` gets each recognised word, punctuation included.
    pub fn feed(&mut self, pcm: &[f32], on_word: &mut dyn FnMut(String)) -> Result<()> {
        self.pending.extend_from_slice(pcm);
        let steps = self.pending.len() / STEP;
        for i in 0..steps {
            let chunk = &self.pending[i * STEP..(i + 1) * STEP];
            let tensor = Tensor::new(chunk, &self.device)?.reshape((1, 1, ()))?;
            let messages = self.state.step_pcm(tensor, None, &().into(), |_, _, _| ())?;
            for message in messages {
                if let moshi::asr::AsrMsg::Word { tokens, .. } = message {
                    let word = self.tokenizer.decode_piece_ids(&tokens).unwrap_or_default();
                    if !word.trim().is_empty() {
                        on_word(word);
                    }
                }
            }
        }
        self.pending.drain(..steps * STEP);
        Ok(())
    }

    /// Ends the utterance: flushes the words still inside the model's delay.
    pub fn finish(&mut self, on_word: &mut dyn FnMut(String)) -> Result<()> {
        let padding = (STEP - self.pending.len() % STEP) % STEP + self.flush_suffix;
        self.feed(&vec![0.0; padding], on_word)
    }
}

fn select_device(preference: &str) -> Result<Device> {
    Ok(match preference {
        "cpu" => Device::Cpu,
        "cuda" => Device::new_cuda(0).context("CUDA unavailable (build with --features cuda)")?,
        "metal" => Device::new_metal(0).context("Metal unavailable (build with --features metal)")?,
        _ if candle::utils::cuda_is_available() => Device::new_cuda(0)?,
        _ if candle::utils::metal_is_available() => Device::new_metal(0)?,
        _ => Device::Cpu,
    })
}
