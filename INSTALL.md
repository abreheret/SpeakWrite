**English** | [Français](INSTALL.fr.md)

# Installing SpeakWrite

This guide starts from a clean machine: tools, build, models, first launch.

## In short

```sh
git clone https://github.com/abreheret/SpeakWrite.git
cd SpeakWrite
scripts\build.cmd                    # Windows
./scripts/build.sh                   # Linux, macOS
target\release\speakwrite.exe        # Windows; elsewhere: ./target/release/speakwrite
```

On first launch, the app downloads its models, about 3.5 GB. The tray icon turns from grey to blue once reading is ready.

Nothing has to be downloaded by hand, and no account is needed, except to read with your own voice (see [section 5](#5-reading-with-your-own-voice-optional)).

## 1. Requirements

| | Windows | Linux | macOS |
|---|---|---|---|
| System | Windows 10 or 11, x64 | x86_64 or aarch64 | Intel or Apple silicon |
| Status | ✅ Tested | ⏳ Untested | ✅ Tested (Apple silicon) |
| Dictation on the GPU | NVIDIA (CUDA) | NVIDIA (CUDA) | Apple silicon (Metal) |

Windows on ARM is not supported: there is no pocket-tts binary for it.

Hardware:
- **RAM**: 8 GB minimum, 16 GB recommended. The app uses about 2.2 GB: 0.7 GB for itself and 0.7 GB per reading language.
- **GPU**: optional. Dictation takes about 2.6 GB of video memory with CUDA. Without a GPU it runs on the CPU, more slowly (not measured).
- **Reading**: always runs on the CPU, about 6× faster than real time.
- **Disk**:
  - about 3.5 GB for the models;
  - about 8 GB while building (the `target/` folder, which can be deleted afterwards);
  - 67 MB for the executable.
- **Network**: only on first launch (GitHub and Hugging Face). After that, models are read from the local cache.

## 2. Installing the tools

You need:
- Git;
- Rust 1.88 or newer;
- CMake 3.5 or newer (CMake 4 is fine);
- a C/C++ compiler.

CUDA is optional.

### Windows

In a PowerShell terminal:

```powershell
winget install Git.Git
winget install Kitware.CMake
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive --wait"
```

The last package installs Microsoft's C++ compiler (the "Desktop development with C++" workload). A full Visual Studio 2022 install with that workload also works.

**Optional, for dictation on an NVIDIA GPU:**
- install an up-to-date NVIDIA driver;
- install the [CUDA Toolkit 12.x](https://developer.nvidia.com/cuda-toolkit-archive), tested with 12.6;
- check in a new terminal that `nvcc --version` answers.

Close and reopen the terminal so that the `PATH` is updated, then check:

```powershell
rustc --version   # 1.88 or newer
cmake --version
```

### Linux (Debian, Ubuntu)

```sh
sudo apt install build-essential cmake pkg-config git curl libssl-dev \
    libgtk-3-dev libayatana-appindicator3-dev libxdo-dev libasound2-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On other distributions, install the equivalent packages: C++ compiler, CMake, pkg-config, and the development packages of OpenSSL, GTK 3, Ayatana AppIndicator, libxdo and ALSA.

Optional: CUDA Toolkit 12.x and an NVIDIA driver, with `nvcc` on the `PATH`.

### macOS

```sh
xcode-select --install
brew install cmake
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The Rust installer puts `cargo` and `rustc` in `~/.cargo/bin`, which is not yet on the `PATH` of the current terminal. Load it, then check:

```sh
source "$HOME/.cargo/env"
rustc --version   # 1.88 or newer
cargo --version
```

If `cargo` is still not found in a new terminal, add the line to your shell configuration (zsh is the default shell on macOS):

```sh
echo 'source "$HOME/.cargo/env"' >> ~/.zshrc
```

On Apple silicon, dictation uses Metal automatically.

## 3. Getting the code and building

```sh
git clone https://github.com/abreheret/SpeakWrite.git
cd SpeakWrite
scripts\build.cmd          # Windows
./scripts/build.sh         # Linux, macOS
```

On Linux and macOS, if the terminal answers `permission denied: ./scripts/build.sh`, make the script executable, then run it again:

```sh
chmod +x scripts/build.sh
```

The scripts pick the acceleration themselves:
- **Windows**: `build.cmd` loads the Visual Studio environment, then builds with CUDA if `nvcc` is found.
- **Linux**: `build.sh` builds with CUDA if `nvcc` is found.
- **macOS**: `build.sh` builds with Metal.
- **Otherwise**: dictation runs on the CPU.

The `legacy/` and `spikes/` folders are not needed.

The first build takes several minutes, longer with CUDA, which compiles its kernels. Later builds are fast. The repository's `Cargo.lock` pins the dependency versions: you build exactly the same ones as the author.

Without the scripts:

```sh
cargo build --release -p speakwrite                    # CPU
cargo build --release -p speakwrite --features cuda    # NVIDIA (Windows: from an "x64 Native Tools" terminal)
cargo build --release -p speakwrite --features metal   # Apple silicon
```

To build with CUDA on a machine without an NVIDIA GPU, give the target GPU generation, for example `CUDA_COMPUTE_CAP=89` for an RTX 40xx or `86` for an RTX 30xx.

## 4. The models

Everything is downloaded automatically on first launch:

| Item | Role | Size | Source | Account required |
|---|---|---|---|---|
| `pocket-tts` 3.3.0 | Local reading server | 22 MB | [GitHub, kyutai-labs/pocket-tts](https://github.com/kyutai-labs/pocket-tts/releases/tag/v4.0.1), SHA-256 verified | No |
| Pocket TTS, one model per language | Reading with the built-in voices | About 430 MB per language | [`kyutai/pocket-tts-without-voice-cloning`](https://huggingface.co/kyutai/pocket-tts-without-voice-cloning) | No |
| Pocket TTS with cloning | Reading with your voice | About 300 MB per language | [`kyutai/pocket-tts`](https://huggingface.co/kyutai/pocket-tts) | Yes, terms to accept |
| Kyutai STT `stt-1b-en_fr-candle` | Dictation | 2.4 GB | [`kyutai/stt-1b-en_fr-candle`](https://huggingface.co/kyutai/stt-1b-en_fr-candle) | No |
| Language detection (`lingua`) | Picks the reading model | Built into the executable | — | — |

By default two languages are loaded: French and English. French is ready first; English then loads in the background.

Locations:

| | Models (Hugging Face cache) | pocket-tts binary, logs |
|---|---|---|
| Windows | `%USERPROFILE%\.cache\huggingface\hub` | `%LOCALAPPDATA%\SpeakWrite\data\` |
| Linux | `~/.cache/huggingface/hub` | `~/.local/share/speakwrite/` |
| macOS | `~/.cache/huggingface/hub` | `~/Library/Application Support/SpeakWrite/` |

The `HF_HOME` environment variable moves the Hugging Face cache, for example to another drive.

To download everything ahead of time, for example before working offline:
1. launch the app once;
2. wait for the blue icon, then one more minute while English finishes loading.

## 5. Reading with your own voice (optional)

Voice cloning requires access to the `kyutai/pocket-tts` repository, which is free but comes with terms:

1. Create an account on [huggingface.co](https://huggingface.co).
2. Open [kyutai/pocket-tts](https://huggingface.co/kyutai/pocket-tts) and accept the terms.
3. Save an access token on the machine:
   ```sh
   uvx --from huggingface_hub hf auth login
   ```
   This command needs [uv](https://docs.astral.sh/uv/). Alternative: `pip install -U huggingface_hub`, then `hf auth login`. The token is saved in the Hugging Face cache, where pocket-tts and SpeakWrite find it.
4. Tray menu → `Voice` → `Add a voice…`, then pick 10 to 30 seconds of natural speech (WAV, MP3, M4A, FLAC or OGG).

Only use your own voice, or the voice of someone who gave explicit consent.

Without access, the voice engine does not start and the error says `We could not download the weights for the model with voice cloning`. The built-in voices always work.

## 6. First launch and checks

1. Run `target/release/speakwrite`. The icon appears in the system tray: grey while starting, blue once reading is ready.
2. **Reading**: select text in any application, then press **F9**.
3. **Dictation**: click in a text field, hold **F8**, speak, then release.

All settings are in the tray menu. It starts in French: click `Menu in English` to switch it to English. The configuration file opens from `Advanced settings (file)…`. Hotkeys and settings are described in the [README](README.md#usage).

Diagnostic commands:

```sh
speakwrite --say "Bonjour"                 # speaks a sentence and measures latency
speakwrite --normalize "Appuie sur F9"     # prints the text as it will be read, with its language
speakwrite --transcribe audio.wav          # runs a file through the dictation pipeline
```

On Windows, the release executable has no console. To see its output, pipe it:
- in PowerShell: `.\target\release\speakwrite.exe --say "Bonjour" | Out-Host`;
- in cmd: `target\release\speakwrite.exe --say Bonjour | more`.

## 7. Common problems

| Symptom | Cause and fix |
|---|---|
| `Visual Studio C++ build tools not found` | Install the "Desktop development with C++" workload (section 2) |
| `nvcc not found: dictation will run on the CPU` | CUDA is missing or not on the `PATH`. Not blocking: dictation runs on the CPU |
| A hotkey does nothing; the logs say `cannot register hotkey` | Another app already uses the key (AutoHotkey, screen capture tools…). Close it, or change the key in `[hotkeys]` |
| `We could not download the weights for the model with voice cloning` | Cloned voice without access to `kyutai/pocket-tts`, or token not saved: see section 5, or pick a built-in voice |
| Very long first launch | Model download (about 3.5 GB). Progress is in the logs, menu → `Open the logs` |
| Dictation types nothing into some windows (Windows) | Windows running as administrator reject simulated keystrokes. Run SpeakWrite as administrator too, or use paste mode (`Dictation` → `Paste everything on release`) |
| Linux: no icon | GNOME needs the AppIndicator extension. On Wayland, global hotkeys and simulated typing depend on the desktop; X11 works |
| macOS: hotkeys or dictation do nothing | Allow SpeakWrite in System Settings → Privacy & Security: "Accessibility", "Input Monitoring", "Microphone" |
| macOS: `ld: tapi error: malformed file` … `unknown architecture` | The Command Line Tools linker is older than the newest SDK installed: see below |

### macOS: `tapi error: malformed file` when linking

The build fails with errors like:

```
ld: multiple errors: tapi error: malformed file
/Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk/usr/lib/libSystem.B.tbd:4:20: error: unknown architecture
```

By default the build uses the newest SDK in `/Library/Developer/CommandLineTools/SDKs/`, here `MacOSX27.0.sdk`. The linker (`ld`) installed with it is older and does not understand that SDK. The project code is not at fault.

**Fix:** update the Command Line Tools so that the linker matches the SDK:

```sh
softwareupdate --list      # then install the Command Line Tools update offered
```

**Workaround:** build against an older SDK. List the available ones, then point `SDKROOT` to one, for example 26.5:

```sh
ls /Library/Developer/CommandLineTools/SDKs/
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
./scripts/build.sh
```

`export` only applies to the current terminal. To keep it, add the line to `~/.zshrc`:

```sh
echo 'export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk' >> ~/.zshrc
```

Remove that line once the Command Line Tools are updated, otherwise the build stays on the old SDK.

### Logs

The logs are in `logs/`, next to the pocket-tts binary (section 4):
- `speakwrite.log.<date>` for the app;
- `pocket-tts-<language>.log` for each reading server.

## Uninstalling

Delete:
- the repository folder;
- the configuration: `%APPDATA%\SpeakWrite` on Windows, `~/.config/speakwrite` on Linux, `~/Library/Application Support/SpeakWrite` on macOS;
- the data: `%LOCALAPPDATA%\SpeakWrite` on Windows, `~/.local/share/speakwrite` on Linux;
- in the Hugging Face cache, the `models--kyutai--pocket-tts*` and `models--kyutai--stt-1b-en_fr-candle` folders.
