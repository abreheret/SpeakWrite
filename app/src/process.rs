//! Child processes that must not outlive SpeakWrite, even after a crash.

use std::process::{Child, Command};

use anyhow::Result;

/// Spawns `command` so that it dies with this process:
/// - Windows: job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, no console window;
/// - Linux: `PR_SET_PDEATHSIG` (sent when the spawning thread exits);
/// - macOS: no OS mechanism; the child is only killed on a normal exit (`Drop`).
pub fn spawn_tied(command: &mut Command) -> Result<Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: prctl is async-signal-safe, as required between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let child = command.spawn()?;
    #[cfg(windows)]
    if let Err(e) = windows_job::kill_on_close(&child) {
        tracing::warn!("cannot tie child process to SpeakWrite: {e:#}");
    }
    Ok(child)
}

#[cfg(windows)]
mod windows_job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use anyhow::{Result, bail};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    pub fn kill_on_close(child: &Child) -> Result<()> {
        // SAFETY: plain Win32 calls on handles we own. The job handle is deliberately leaked:
        // Windows closes it when SpeakWrite exits, which kills the processes in the job.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                bail!("CreateJobObjectW: {}", std::io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                bail!("SetInformationJobObject: {}", std::io::Error::last_os_error());
            }
            if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
                bail!("AssignProcessToJobObject: {}", std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
}
