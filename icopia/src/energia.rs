//! Impide que el equipo se suspenda mientras viva el guardián.
//! Windows: `SetThreadExecutionState` (por hilo). macOS: `caffeinate -i -w <pid>` como proceso hijo
//! (`-w` hace que termine solo si iCopy muere, así nunca queda el Mac sin poder dormir).

#[cfg(windows)]
pub struct SinSuspension {
    _no_send: std::marker::PhantomData<*const ()>,
}

#[cfg(windows)]
impl SinSuspension {
    pub fn activar() -> SinSuspension {
        use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        }
        SinSuspension { _no_send: std::marker::PhantomData }
    }
}

#[cfg(windows)]
impl Drop for SinSuspension {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(not(windows))]
pub struct SinSuspension {
    hijo: Option<std::process::Child>,
}

#[cfg(not(windows))]
impl SinSuspension {
    pub fn activar() -> SinSuspension {
        let hijo = std::process::Command::new("caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok();
        SinSuspension { hijo }
    }

    #[cfg(test)]
    fn pid(&self) -> Option<u32> {
        self.hijo.as_ref().map(|h| h.id())
    }
}

#[cfg(not(windows))]
impl Drop for SinSuspension {
    fn drop(&mut self) {
        if let Some(mut h) = self.hijo.take() {
            let _ = h.kill();
            let _ = h.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn activar_y_soltar() {
        let g = super::SinSuspension::activar();
        drop(g);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn caffeinate_vive_mientras_la_guarda() {
        let vivo = |pid: u32| std::process::Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(false);
        let g = super::SinSuspension::activar();
        let pid = g.pid().expect("caffeinate lanzado");
        assert!(vivo(pid));
        drop(g);
        assert!(!vivo(pid), "al soltar la guarda, caffeinate termina");
    }
}
