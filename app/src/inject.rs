//! Writes dictated text into the focused field of the active application.

use std::time::Duration;

use anyhow::{Context, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

pub struct Injector {
    enigo: Enigo,
    // Kept alive: on X11 the clipboard content is served by this process.
    clipboard: arboard::Clipboard,
}

impl Injector {
    pub fn new() -> Result<Self> {
        Ok(Self {
            enigo: Enigo::new(&Settings::default())
                .context("cannot simulate keystrokes (on macOS: grant Accessibility permission)")?,
            clipboard: arboard::Clipboard::new().context("cannot access the clipboard")?,
        })
    }

    /// Types `text` as keystrokes (Unicode, so accents work whatever the keyboard layout).
    pub fn type_text(&mut self, text: &str) -> Result<()> {
        self.enigo.text(text).context("cannot type the dictated text")
    }

    /// Pastes `text` through the clipboard, then restores the previous clipboard text.
    /// Faster for long texts, and works in apps that ignore synthetic Unicode input.
    pub fn paste(&mut self, text: &str) -> Result<()> {
        let previous = self.clipboard.get_text().ok();
        self.clipboard.set_text(text.to_string()).context("cannot fill the clipboard")?;
        std::thread::sleep(Duration::from_millis(30));

        #[cfg(target_os = "macos")]
        let (modifier, key) = (Key::Meta, Key::Other(0x09)); // kVK_ANSI_V
        #[cfg(windows)]
        let (modifier, key) = (Key::Control, Key::Other(0x56)); // VK_V
        #[cfg(all(unix, not(target_os = "macos")))]
        let (modifier, key) = (Key::Control, Key::Unicode('v'));
        self.enigo.key(modifier, Direction::Press)?;
        let result = self.enigo.key(key, Direction::Click);
        self.enigo.key(modifier, Direction::Release)?;
        result?;

        // Give the target application time to read the clipboard before restoring it.
        std::thread::sleep(Duration::from_millis(150));
        if let Some(previous) = previous {
            let _ = self.clipboard.set_text(previous);
        }
        Ok(())
    }
}
