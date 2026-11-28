//! Start with the user session: a per-user entry that launches the running executable.
//!
//! - Windows: a value of the `Run` registry key of the current user;
//! - macOS: a LaunchAgent in `~/Library/LaunchAgents`;
//! - Linux: an XDG autostart file in `~/.config/autostart`.
//!
//! The entry itself is the setting, nothing goes into the config file. It counts as enabled only
//! when it launches this very executable: a copy moved elsewhere shows it off, and enabling it
//! again points the entry at the new place.

use std::path::Path;

use anyhow::{Context, Result};

/// This executable starts with the session.
pub fn is_enabled() -> bool {
    std::env::current_exe().is_ok_and(|exe| platform::current().is_some_and(|entry| entry == expected(&exe)))
}

pub fn set(enabled: bool) -> Result<()> {
    if enabled {
        let exe = std::env::current_exe().context("cannot find the path of SpeakWrite")?;
        platform::write(&expected(&exe))?;
    } else {
        platform::remove()?;
    }
    tracing::info!(enabled, "start with the session");
    Ok(())
}

#[cfg(windows)]
fn expected(exe: &Path) -> String {
    run_command(exe)
}

#[cfg(target_os = "macos")]
fn expected(exe: &Path) -> String {
    launch_agent(exe)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn expected(exe: &Path) -> String {
    desktop_entry(exe)
}

/// The command line of the `Run` value, quoted for paths with spaces.
#[cfg(any(windows, test))]
fn run_command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(any(target_os = "macos", test))]
const LAUNCH_AGENT_LABEL: &str = "com.github.abreheret.speakwrite";

/// A LaunchAgent started once at login, not restarted when the user quits the app.
#[cfg(any(target_os = "macos", test))]
fn launch_agent(exe: &Path) -> String {
    let program = exe.to_string_lossy().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCH_AGENT_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{program}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    )
}

/// An XDG autostart entry. In `Exec`, the path is quoted, with the escapes of the Desktop Entry
/// specification: `\"`, `` \` ``, `\$` and `\\` inside quotes, each backslash then doubled
/// again as in any string value, and `%%` for `%`.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn desktop_entry(exe: &Path) -> String {
    let mut exec = String::from("\"");
    for c in exe.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' => {
                exec.push_str(r"\\");
                exec.push(c);
            }
            '\\' => exec.push_str(r"\\\\"),
            '%' => exec.push_str("%%"),
            c => exec.push(c),
        }
    }
    exec.push('"');
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=SpeakWrite\n\
         Comment=Read the selected text aloud and dictate\n\
         Exec={exec}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(windows)]
mod platform {
    use std::ptr::null_mut;

    use anyhow::{Result, bail};
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "SpeakWrite";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub fn current() -> Option<String> {
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        let mut size = 0u32;
        // SAFETY: valid NUL-terminated strings; a null buffer only asks for the size in bytes.
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size)
        };
        if status != ERROR_SUCCESS || size == 0 {
            return None;
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        // SAFETY: `buffer` holds `size` bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }

    pub fn write(command: &str) -> Result<()> {
        let (key, value, data) = (wide(RUN_KEY), wide(VALUE), wide(command));
        // SAFETY: valid NUL-terminated strings; `data` is `data.len() * 2` bytes long.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if status != ERROR_SUCCESS {
            bail!("cannot write HKEY_CURRENT_USER\\{RUN_KEY}\\{VALUE} (error {status})");
        }
        Ok(())
    }

    pub fn remove() -> Result<()> {
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        // SAFETY: valid NUL-terminated strings.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            bail!("cannot delete HKEY_CURRENT_USER\\{RUN_KEY}\\{VALUE} (error {status})");
        }
        Ok(())
    }
}

/// macOS and Linux: one file.
#[cfg(unix)]
mod platform {
    use std::path::PathBuf;

    use anyhow::{Context, Result};

    fn entry_path() -> Option<PathBuf> {
        let dirs = directories::BaseDirs::new()?;
        #[cfg(target_os = "macos")]
        let path = dirs.home_dir().join("Library/LaunchAgents").join(format!("{}.plist", super::LAUNCH_AGENT_LABEL));
        #[cfg(not(target_os = "macos"))]
        let path = dirs.config_dir().join("autostart").join("speakwrite.desktop");
        Some(path)
    }

    pub fn current() -> Option<String> {
        std::fs::read_to_string(entry_path()?).ok()
    }

    pub fn write(content: &str) -> Result<()> {
        let path = entry_path().context("no home folder")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        std::fs::write(&path, content).with_context(|| format!("cannot write {}", path.display()))
    }

    pub fn remove() -> Result<()> {
        let path = entry_path().context("no home folder")?;
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(e).with_context(|| format!("cannot delete {}", path.display()))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_command_is_quoted() {
        assert_eq!(
            run_command(Path::new(r"C:\Program Files\SpeakWrite\speakwrite.exe")),
            r#""C:\Program Files\SpeakWrite\speakwrite.exe""#
        );
    }

    #[test]
    fn launch_agent_escapes_xml() {
        let plist = launch_agent(Path::new("/Users/a&b/SpeakWrite <dev>/speakwrite"));
        assert!(plist.contains("<string>/Users/a&amp;b/SpeakWrite &lt;dev&gt;/speakwrite</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(!plist.contains("KeepAlive"));
    }

    #[test]
    fn desktop_entry_quotes_exec() {
        let entry = desktop_entry(Path::new("/home/me/mes outils/speakwrite"));
        assert!(entry.contains("\nExec=\"/home/me/mes outils/speakwrite\"\n"));
        let entry = desktop_entry(Path::new(r#"/opt/100%/$dir/a"b\c"#));
        assert!(entry.contains(r#"Exec="/opt/100%%/\\$dir/a\\"b\\\\c""#), "{entry}");
    }
}
