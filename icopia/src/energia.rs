//! Impide que Windows se suspenda mientras viva el guardián (por hilo).
use std::marker::PhantomData;
use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};

pub struct SinSuspension {
    _no_send: PhantomData<*const ()>,
}

impl SinSuspension {
    pub fn activar() -> SinSuspension {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        }
        SinSuspension { _no_send: PhantomData }
    }
}

impl Drop for SinSuspension {
    fn drop(&mut self) {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
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
}
