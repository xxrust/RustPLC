#![cfg(target_os = "none")]

use super::PicoIo;

/// Motion subsystem hook points.
///
/// This is intentionally a small "lifecycle-shaped" surface so later iterations can add:
/// - PIO step generation (Pulse/Dir/EN)
/// - PIO quadrature decoding / high-rate counting (AB encoder)
/// without growing `main.rs` into an unreviewable monolith.
pub(super) struct Motion;

impl Motion {
    pub(super) fn initialize(_io: &mut PicoIo) -> Self {
        // No-op in US-003. Later stories will allocate/configure PIO SMs here.
        Self
    }

    /// Update motion feedback signals before runtime evaluation.
    pub(super) fn update_in(&mut self, _io: &mut PicoIo) {
        // No-op in US-003. Later stories will publish encoder count/speed/dir here.
    }

    /// Apply motion outputs after runtime evaluation.
    pub(super) fn update_out(&mut self, _io: &mut PicoIo) {
        // No-op in US-003. Later stories will consume motion commands here.
    }

    /// Best-effort controlled-stop path on runtime failure.
    pub(super) fn finalize_on_error(&mut self, _io: &mut PicoIo) {
        // No-op in US-003. Later stories may stop PIO SMs and de-energize motion outputs.
    }
}

