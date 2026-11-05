//! Gets the text selected in the active application.
//!
//! Simulates the platform copy shortcut, waits for the clipboard to change, then puts the
//! previous text back. On Linux the primary selection is read first, without copying.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::config::SelectionConfig;

pub struct SelectionReader {
    // Kept alive for the whole app: on X11, restored clipboard contents are served by this
    // process and vanish when the clipboard object is dropped.
    clipboard: arboard::Clipboard,
    enigo: Enigo,
    config: SelectionConfig,
}

impl SelectionReader {
    pub fn new(config: SelectionConfig) -> Result<Self> {
        let clipboard = arboard::Clipboard::new().context("cannot access the clipboard")?;
        let enigo = Enigo::new(&Settings::default())
            .context("cannot simulate keystrokes (on macOS: grant Accessibility permission)")?;
        Ok(Self { clipboard, enigo, config })
    }

    /// The selected text, or the clipboard content if nothing is selected and the fallback is
    /// enabled. `None` when there is nothing to read.
    pub fn capture(&mut self) -> Result<Option<String>> {
        #[cfg(target_os = "linux")]
        if self.config.linux_primary_selection
            && let Some(text) = self.primary_selection()
        {
            return Ok(Some(text));
        }

        let previous = self.clipboard.get_text().ok();
        self.send_copy_shortcut()?;

        // Without a selection, the clipboard does not change: the timeout tells.
        let deadline = Instant::now() + Duration::from_millis(self.config.copy_timeout_ms);
        let mut copied = None;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
            if let Ok(text) = self.clipboard.get_text()
                && !text.trim().is_empty()
                && Some(&text) != previous.as_ref()
            {
                copied = Some(text);
                break;
            }
        }

        if copied.is_some()
            && self.config.restore_clipboard
            && let Some(previous) = &previous
        {
            // Let the source application finish its clipboard work first.
            std::thread::sleep(Duration::from_millis(50));
            if let Err(e) = self.clipboard.set_text(previous.clone()) {
                tracing::warn!("cannot restore the clipboard: {e}");
            }
        }

        let text = match copied {
            Some(text) => Some(text),
            // Same text selected as already copied, or no selection at all.
            None if self.config.fallback_to_clipboard => previous,
            None => None,
        };
        Ok(text.filter(|t| !t.trim().is_empty()))
    }

    fn send_copy_shortcut(&mut self) -> Result<()> {
        // Virtual key codes rather than characters, so that the shortcut works whatever the
        // keyboard layout (same approach as Handy).
        #[cfg(target_os = "macos")]
        let (modifier, key) = (Key::Meta, Key::Other(0x08)); // kVK_ANSI_C
        #[cfg(windows)]
        let (modifier, key) = (Key::Control, Key::Other(0x43)); // VK_C
        #[cfg(all(unix, not(target_os = "macos")))]
        let (modifier, key) = (Key::Control, Key::Unicode('c'));

        // Shift may still be held from a hotkey like Shift+F9: Ctrl+Shift+C is another
        // shortcut (the inspector in browsers).
        self.enigo.key(Key::Shift, Direction::Release)?;
        self.enigo.key(modifier, Direction::Press)?;
        let result = self.enigo.key(key, Direction::Click);
        // Always release the modifier, even if the click failed.
        self.enigo.key(modifier, Direction::Release)?;
        Ok(result?)
    }

    #[cfg(target_os = "linux")]
    fn primary_selection(&mut self) -> Option<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        self.clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
            .ok()
            .filter(|t| !t.trim().is_empty())
    }
}
