//! Kyutai Pocket TTS, through its official local server (`pocket-tts serve`).
//!
//! The server loads the model once and streams a WAV while generating; closing the connection
//! stops the generation, which is how a reading is cancelled.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::{AudioChunk, TtsEngine, install, wav};
use crate::config::{Paths, PocketConfig};

/// First launch downloads the model weights (a few hundred MB).
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub struct PocketTts {
    child: Child,
    language: String,
    base_url: String,
    agent: ureq::Agent,
    log_path: PathBuf,
    ready: bool,
}

impl PocketTts {
    /// Starts the server of `language` (a model name, see `pocket-tts serve --help`)
    /// without waiting for it to load: see [`TtsEngine::wait_ready`].
    pub fn spawn(config: &PocketConfig, language: &str, paths: &Paths) -> Result<Self> {
        let binary = match config.binary.trim() {
            "" => install::ensure_binary(&paths.bin_dir)?,
            path => PathBuf::from(path),
        };
        let port = free_port()?;
        let log_path = paths.log_dir.join(format!("pocket-tts-{language}.log"));
        let log = std::fs::File::create(&log_path)
            .with_context(|| format!("cannot create {}", log_path.display()))?;

        let mut command = Command::new(&binary);
        command
            .arg("serve")
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--language", language]);
        if let Some(voice) = config.voice_argument(paths)? {
            tracing::info!(voice = %voice.to_string_lossy(), "voice");
            command.arg("--default-voice").arg(&voice);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        tracing::info!(binary = %binary.display(), port, language, "starting pocket-tts");
        let child = crate::process::spawn_tied(&mut command)
            .with_context(|| format!("cannot start {}", binary.display()))?;

        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(2)))
            .build()
            .new_agent();
        Ok(Self {
            child,
            language: language.to_string(),
            base_url: format!("http://127.0.0.1:{port}"),
            agent,
            log_path,
            ready: false,
        })
    }
}

impl TtsEngine for PocketTts {
    fn wait_ready(&mut self) -> Result<()> {
        if self.ready {
            return Ok(());
        }
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let health = format!("{}/health", self.base_url);
        loop {
            if let Some(status) = self.child.try_wait()? {
                bail!("pocket-tts ({}) exited ({status}):\n{}", self.language, log_tail(&self.log_path));
            }
            if self.agent.get(&health).call().is_ok() {
                tracing::info!(language = %self.language, "pocket-tts ready");
                self.ready = true;
                return Ok(());
            }
            if Instant::now() > deadline {
                bail!(
                    "pocket-tts ({}) not ready after {STARTUP_TIMEOUT:?}:\n{}",
                    self.language,
                    log_tail(&self.log_path)
                );
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn is_ready(&self) -> bool {
        self.ready
    }
    fn synthesize(
        &mut self,
        text: &str,
        is_cancelled: &dyn Fn() -> bool,
        on_audio: &mut dyn FnMut(AudioChunk),
    ) -> Result<()> {
        let response = self
            .agent
            .post(format!("{}/tts", self.base_url))
            .send_form([("text", text)])
            .context("pocket-tts request failed")?;
        let mut body = response.into_body().into_reader();
        let format = wav::read_header(&mut body)?;

        let mut buffer = vec![0u8; 8192];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            if is_cancelled() {
                // Dropping the reader closes the connection: the server stops generating.
                return Ok(());
            }
            let n = body.read(&mut buffer).context("pocket-tts stream interrupted")?;
            if n == 0 {
                return Ok(());
            }
            pending.extend_from_slice(&buffer[..n]);
            let mut samples = Vec::new();
            let used = wav::decode(&format, &pending, &mut samples);
            pending.drain(..used);
            if !samples.is_empty() {
                on_audio(AudioChunk {
                    sample_rate: format.sample_rate,
                    channels: format.channels,
                    samples,
                });
            }
        }
    }
}

impl Drop for PocketTts {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").context("no free local port")?;
    Ok(listener.local_addr()?.port())
}

fn log_tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(15)..].join("\n")
}
