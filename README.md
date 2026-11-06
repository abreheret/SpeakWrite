**English** | [Français](README.fr.md)

# SpeakWrite

[![Windows](https://github.com/abreheret/SpeakWrite/actions/workflows/build-windows.yml/badge.svg?branch=master)](https://github.com/abreheret/SpeakWrite/actions/workflows/build-windows.yml)
[![Linux](https://github.com/abreheret/SpeakWrite/actions/workflows/build-linux.yml/badge.svg?branch=master)](https://github.com/abreheret/SpeakWrite/actions/workflows/build-linux.yml)
[![macOS](https://github.com/abreheret/SpeakWrite/actions/workflows/build-macos.yml/badge.svg?branch=master)](https://github.com/abreheret/SpeakWrite/actions/workflows/build-macos.yml)
[![Release](https://img.shields.io/github/v/release/abreheret/SpeakWrite)](https://github.com/abreheret/SpeakWrite/releases/latest)

A **local** desktop tool for Windows, Linux and macOS that works both ways:

- **Reading**: select text anywhere, press a hotkey, and it is read aloud, optionally saved as MP3.
- **Dictation**: hold a key, speak, and the text is typed into the focused field.

Both directions use the open-source models of [Kyutai](https://kyutai.org) and run on the machine, with no online service.

Reading detects the language of the text, line by line: French and English by default. Each language is read by its own Pocket TTS model, with the same voice, your own included. The spelling-out of numbers and the menu labels target **French**, the author's language.

## Project status

Phase 4: **polish**.

| Component | Status | Windows | Linux | macOS |
|---|---|---|---|---|
| Reading: F9 read, F10 pause, Shift+F10 stop (Pocket TTS, cloned voice) | ✅ Working | ✅ | ⏳ Untested | ✅ |
| MP3 recording of readings (Shift+F9, or every reading) | ✅ Working | ✅ | ⏳ | ⏳ |
| Dictation: hold F8 (Kyutai STT, streaming) | ✅ Working | ✅ CUDA | ⏳ (CUDA / CPU) | ✅ Metal |
| Spike: Kyutai TTS 1.6B `tts-1.6b-en_fr` (Python) | ✅ Validated | ✅ CUDA | ⏳ (CUDA) | ⏳ (MLX) |

`legacy/` holds the previous prototype (AutoHotkey + PowerShell + edge-tts, Windows only).

## How it works

```
speakwrite (Rust, single executable, system tray icon)
├─ READING   : hotkey → copy the selection → Kyutai Pocket TTS → speakers (+ optional MP3)
└─ DICTATION : hold key → microphone → Kyutai STT (streaming) → text typed into the focused field
```

Rust building blocks:

| Role | Crate |
|---|---|
| Event loop | `tao` |
| Global hotkeys (press and release) | `global-hotkey` |
| Tray icon, menu, dialogs | `tray-icon` (+ `muda`), `rfd` |
| Clipboard, simulated keystrokes | `arboard`, `enigo` |
| Audio playback, microphone capture | `rodio`, `cpal` |
| Voice: official local server `pocket-tts serve`, driven over HTTP | `ureq` |
| Language of the text to read | `lingua` |
| Dictation: STT inference | `moshi` + `candle` (CUDA / Metal / CPU) |
| Audio decoding, MP3 encoding | `symphonia`, `mp3lame-encoder` |
| Configuration | `serde`, `toml`, `toml_edit`, `directories` |

Layout of `app/src`:

| Module | Role |
|---|---|
| `main.rs` | Event loop, hotkeys, menu, diagnostic commands |
| `reader.rs` | Reading thread. Each request carries a generation number: a new F9 or a stop cancels the previous one |
| `selection.rs` | Simulated Ctrl+C (Cmd+C on macOS), wait for the clipboard, restore it. Primary selection on Linux |
| `language.rs` | Splits the text into runs of lines in one language. Short lines (titles) follow the next long line; undecidable texts use the first language |
| `textnorm/` | French spelling-out of numbers, times, ordinals, %, €, F1–F24 keys; pronunciation dictionary. Other languages: Markdown and spaces only, their models read digits themselves |
| `tts/` | `TtsEngine` trait, one engine per language started on demand, Pocket TTS engine, verified binary download, streamed WAV reader |
| `tray.rs` | Tray icon and menu (voices, language, speed, MP3, dictation), rebuilt on every change |
| `voices.rs` | Catalogue: built-in voices and recordings in the `voices/` folder, import of a new file, renaming |
| `voiceprep.rs` | Prepares each recording for cloning: mono, long pauses shortened, the best 25 s of speech, high-pass filter, level normalization, cached. Otherwise Pocket TTS uses the whole file: 4 minutes led to a 73 GB allocation and a crash |
| `stretch.rs` | Playback speed without pitch change: streaming WSOLA time-stretch, pure Rust |
| `audio.rs` | Gapless playback of streamed chunks, pause, instant stop |
| `recorder.rs` | MP3 (LAME) encoded during synthesis into a `.part` file, renamed at the end: never a truncated file |
| `dictation.rs` | Dictation thread: the model stays loaded, the microphone opens only while the key is held, capture continues 300 ms after release |
| `stt.rs` | Kyutai STT `stt-1b-en_fr` through the `moshi` crate. Final words, about 0.5 s after they are spoken |
| `mic.rs`, `resample.rs` | Microphone capture, mono mixdown, resampling to 24 kHz (windowed sinc) |
| `inject.rs` | Writing into the focused field: Unicode keystrokes, or paste with clipboard restore |
| `process.rs` | The voice server dies with the app, even on a crash (Windows job object, Linux `PR_SET_PDEATHSIG`) |
| `config.rs` | Commented TOML file, standard per-OS locations, migration of older files |

### Platform constraints

| | Windows | Linux | macOS |
|---|---|---|---|
| Acceleration | CUDA (NVIDIA) | CUDA (NVIDIA) | Metal (Apple silicon) |
| Global hotkeys | OK | X11: OK. Wayland: `GlobalShortcuts` portal, desktop-dependent | "Input Monitoring" and "Accessibility" permissions |
| Text injection | OK, except into windows running as administrator (UIPI) | X11: OK. Wayland: `wtype` / `ydotool`, or clipboard | "Accessibility" permission |
| Tray icon | OK | `libayatana-appindicator` + GTK | OK |
| Microphone | OK | ALSA / PulseAudio / PipeWire | "Microphone" permission |

## Application

### Build and run

**Step-by-step guide, from a clean machine to the first launch: [INSTALL.md](INSTALL.md)**. It covers the tools, the build, the downloaded models, your own voice and common problems.

The scripts enable GPU acceleration for dictation automatically: CUDA when `nvcc` is found, Metal on macOS, CPU otherwise.

```sh
scripts\build.cmd                    # Windows: loads the Visual Studio environment, CUDA if present
./scripts/build.sh                   # Linux / macOS
./target/release/speakwrite          # Windows: target\release\speakwrite.exe
```

Without the scripts: `cargo build --release -p speakwrite --features cuda`, or `--features metal`, or no feature for CPU.

Requirements:
- Rust ≥ 1.88 and CMake;
- Windows: Visual Studio Build Tools (C++); CUDA Toolkit 12.x for GPU dictation;
- Linux: `build-essential cmake pkg-config libssl-dev libgtk-3-dev libayatana-appindicator3-dev libxdo-dev libasound2-dev`.

On first launch, the app:
1. creates the configuration file;
2. downloads the official `pocket-tts` binary (release v4.0.1, SHA-256 verified);
3. starts one local server per reading language (French, then English), each downloading its model weights (about 430 MB);
4. downloads the Kyutai STT dictation model (about 2.4 GB) into the Hugging Face cache.

Icon colors:
- grey: starting;
- blue: ready;
- green: reading;
- amber: paused;
- orange: listening (dictation);
- red: error.

### Usage

| Key | Action |
|---|---|
| **F9** | Reads the text selected in any application. With no selection, reads the clipboard |
| **F10** | Pauses or resumes the reading. Synthesis keeps going while paused, so resuming is instant |
| **Shift+F10** | Stops the reading. Pressing F9 during a reading switches to the new selection |
| **Shift+F9** | Reads the selection **and saves it as MP3**, as heard (voice, speed): `2026-11-05 14-32-10 First words of the text.mp3`. Interrupted readings are not saved |
| **F8**, held | Dictation. Text is typed as you speak, with punctuation and capitalization. Any reading stops, so that the microphone does not hear it |

The **tray menu** covers everything, without editing the configuration file:
- **Voices**:
  - your own recordings;
  - the 27 voices bundled with Pocket TTS;
  - "Add a voice…", with a file picker;
  - "Open the voices folder".

  Switching voices restarts the engines (about 3 s), then plays a sample.
- **Language**: automatic (French and English, line by line), or always one language. Applied at once, with a sample.
- **Speed**: from 0.8× to 2×, applied at once, with a sample.
- **MP3 recording**:
  - save every F9 reading too;
  - open the last recording or the folder;
  - choose the folder (default `Music/SpeakWrite`).
- **Dictation**:
  - type as you speak, or paste everything on release, for apps that ignore simulated keystrokes;
  - add a trailing space after each dictation.
- **Play a sample**, menu language (`Menu in English` / `Menu en français`), advanced settings (file), reload, logs, quit.

Every choice is saved to `config.toml`, keeping the file's comments.

The menu and dialogs are in French or English: `Menu in English` switches to English, `Menu en français` back to French. The choice is stored in `[interface] language`.

### Configuration

| OS | Configuration file | Logs, pocket-tts binary |
|---|---|---|
| Windows | `%APPDATA%\SpeakWrite\config\config.toml` | `%LOCALAPPDATA%\SpeakWrite\data\` |
| macOS | `~/Library/Application Support/SpeakWrite/config.toml` | `~/Library/Application Support/SpeakWrite/` |
| Linux | `~/.config/speakwrite/config.toml` | `~/.local/share/speakwrite/` |

The file is commented. It sets:
- the hotkeys;
- the clipboard behavior;
- the playback speed (`[playback] speed`);
- MP3 recording (`[recording]`: on/off, folder, bitrate from 32 to 160 kbit/s);
- dictation (`[dictation]`: key, insertion mode, device, model);
- the reading language (`[pocket] language`: `"auto"` or a fixed language) and the languages detected in `"auto"` mode (`auto_languages`, French and English by default; also German, Spanish, Italian, Portuguese and Dutch). Each language runs its own model, 0.5 to 0.8 GB of memory;
- the voice.

It also holds the pronunciation dictionary, in the `[pronunciation]` section, applied to French texts.

**To read with your own voice**:
1. Accept the terms on [`kyutai/pocket-tts`](https://huggingface.co/kyutai/pocket-tts), then run `uvx --from huggingface_hub hf auth login`.
2. Tray menu → Voices → "Add a voice…".

Only use your own voice, or the voice of someone who gave explicit consent.

### Diagnostics

```sh
speakwrite --normalize "Appuie sur F9 : 3,5 % en 2026"   # prints the text as it will be read, with its language
speakwrite --say "Bonjour"                               # speaks a sentence and measures latency
speakwrite --transcribe audio.wav                        # runs a file through the dictation pipeline
SPEAKWRITE_SAY_WAV=out.wav speakwrite --say "..."        # also saves the received audio (WAV)
SPEAKWRITE_SAY_MP3=out.mp3 speakwrite --say "..."        # same, as MP3
SPEAKWRITE_LOG=debug speakwrite                          # verbose logs
```

Measurements on Windows 11, RTX 4090:

| Reading (Pocket TTS, CPU, cloned voice) | Dictation (Kyutai STT, CUDA) |
|---|---|
| Engine ready in 3 to 5 s (French), English 3 s later | Model loaded in about 1.4 s once cached |
| First audio after about 100 ms | Words about 0.5 s after speech |
| Generation about 6× faster than real time | Processing about 8× faster than real time; last words about 0.45 s after release |

## Spikes (phase 1)

### Requirements

Common: [Rust](https://rustup.rs), [uv](https://docs.astral.sh/uv/), CMake ≥ 3.5, about 10 GB of disk space for the models (Hugging Face cache).

- **Windows**: Visual Studio 2022 or Build Tools with "Desktop development with C++". CUDA Toolkit 12.x, optional (CPU otherwise).
- **Linux**: `build-essential`, `cmake`, `pkg-config`, `libportaudio2`. CUDA Toolkit 12.x, optional.
- **macOS**: Xcode Command Line Tools, `cmake` (Homebrew). Apple silicon required for MLX.

### Dictation: `spikes/stt-rs`

Kyutai's official example (Rust + Candle). It picks CUDA, then Metal, then the CPU.

```sh
# Windows
spikes\stt-rs\build.cmd
spikes\stt-rs\target\release\kyutai-stt-rs.exe path\to\audio.mp3

# Linux / macOS
./spikes/stt-rs/build.sh
./spikes/stt-rs/target/release/kyutai-stt-rs path/to/audio.mp3
```

Options: `--cpu` (force CPU), `--timestamps` (word timestamps), `--vad` (end-of-turn detection).

### 1.6B reading: `spikes/tts-kyutai`

Kyutai's official scripts. `uv` installs the right build for each OS:
- Windows: PyTorch CUDA 12.6;
- Linux: PyTorch from PyPI (CUDA included);
- macOS: MLX.

```sh
cd spikes/tts-kyutai
uv sync

# Windows / Linux (NVIDIA GPU)
uv run tts_pytorch.py texte_test.txt out.wav
uv run tts_pytorch.py texte_test.txt -          # play directly on the speakers

# macOS (Apple silicon)
uv run tts_mlx.py texte_test.txt out.wav
```

French voices (`--voice`, paths relative to the [`kyutai/tts-voices`](https://huggingface.co/kyutai/tts-voices) repository):
- `unmute-prod-website/developpeuse-3.wav` (default)
- `unmute-prod-website/fabieng-enhanced-v2.wav`
- `cml-tts/fr/*.wav`, about 70 speakers, each also available as `_enhanced` (cleaned up)

### Measurements (Windows 11, RTX 4090)

| Test | Result |
|---|---|
| STT: 16.3 s of speech | 3.6 s in total, **model loading included**. Correct punctuation and capitalization. Errors: rare proper names ("Kyutai" → "QT"), homophones without context ("où" → "ou"). |
| TTS 1.6B: 17.4 s of audio | Generated in 5.6 to 7.9 s, without `torch.compile` (Triton has no Windows build). Numbers and acronyms read correctly: the STT transcript of the output almost matches the source text. |

### Voice cloning

- **TTS 1.6B: not possible.** The module that turns a recording into a voice embedding (`speaker_wavs.compression_model`) is deliberately not released. Only the precomputed voices of `kyutai/tts-voices` can be used.
- **Pocket TTS: possible**, officially, locally, on the CPU. It is the app's engine.

## Roadmap

1. ✅ **Validation**: Kyutai STT, TTS 1.6B, Pocket TTS with your own voice.
2. ✅ **Reading**
   - tray icon, hotkeys, selection capture;
   - spelling-out of numbers and symbols;
   - streamed Pocket TTS;
   - voice and speed from the menu;
   - pause;
   - MP3 recording;
   - automatic language detection (French / English).
3. ✅ **Dictation**: streamed Kyutai STT, text typed into the focused field.
4. ⏸️ **TTS 1.6B in the app**: blocked in pure Rust. The released model uses `demux_second_stream` and a weights-per-step schedule that the `moshi` crate 0.6.1 does not implement, and Kyutai publishes no Rust-converted weights. Options: a Python server started by the app, or a fork of the `moshi` crate.
5. 🔜 **Polish**
   - start with the user session;
   - recover when the audio output changes (headset plugged in);
   - automatic TTS → STT tests on a sentence corpus;
   - upgrade to the latest crates after `rustup update` (tray-icon 0.26 needs Rust 1.90);
   - tests on Linux and macOS;
   - per-OS packages.

## Development

The code was written with the assistance of [Claude Opus 5.5](https://www.anthropic.com/claude) (Anthropic), through Claude Code.

## Licenses

- SpeakWrite code: **MIT**, see [LICENSE](LICENSE).
- MP3 encoder: LAME, through `mp3lame-encoder` / `mp3lame-sys`, **LGPL**, statically linked. The source code stays MIT, but distributing a SpeakWrite binary requires complying with the LGPL for LAME (allowing relinking with another LAME build).
- Language detection: `lingua`, Apache-2.0.
- `app/src/stt.rs` and `spikes/stt-rs`: derived from [kyutai-labs/delayed-streams-modeling](https://github.com/kyutai-labs/delayed-streams-modeling), MIT / Apache-2.0 (license files in `spikes/stt-rs`).
- `spikes/tts-kyutai`: adapted Kyutai scripts, MIT (license file included).
- Kyutai models (`stt-1b-en_fr`, `tts-1.6b-en_fr`, Pocket TTS): **CC-BY 4.0**, credit Kyutai when redistributing.
- Voices from `kyutai/tts-voices`: license specific to each dataset (see the Hugging Face repository).
