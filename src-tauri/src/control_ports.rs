//! Platform boundaries for the Mac-first control core.
//! Implementations are called by the coordinator, never synchronously from OS hooks.
use crate::shared_input::InputCommand;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortError {
    PermissionDenied,
    Unavailable,
    QueueFull,
    SubmissionFailed,
}

pub trait Clock {
    fn now_ms(&self) -> u64;
}

pub struct MonotonicClock(Instant);

impl Default for MonotonicClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for MonotonicClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
}

pub trait CapturePort {
    /// Prepare platform resources while physical input remains local.
    fn prepare_capture(&mut self) -> Result<(), PortError>;
    fn activate_capture(&mut self) -> Result<(), PortError>;
    /// Nonblocking request to the platform owner; must not wait for network/UI.
    fn request_local_restore(&mut self);
}

pub trait FocusPort {
    /// At most once per explicit user request. Failure prevents activation.
    fn prepare_focus(&mut self) -> Result<(), PortError>;
}

pub trait InjectorPort {
    fn readiness(&self) -> Result<(), PortError>;
    /// Success means submission, not that a target application handled it.
    fn post_event(&mut self, event: InputCommand) -> Result<(), PortError>;
}

pub trait TransportPort {
    /// A bounded, nonblocking offer. Never claims delivery or remote injection.
    fn try_send(&mut self, payload: &[u8]) -> Result<(), PortError>;
}

/// Shared readiness policy, testable without loading any platform backend.
pub fn submit_ready<I: InjectorPort>(
    injector: &mut I,
    event: InputCommand,
) -> Result<(), PortError> {
    injector.readiness()?;
    injector.post_event(event)
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    pub struct FakeClock(pub Cell<u64>);
    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0.get()
        }
    }

    #[derive(Default)]
    pub struct FakeCapture {
        pub prepared: usize,
        pub activated: usize,
        pub restored: usize,
        pub failure: Option<PortError>,
    }
    impl CapturePort for FakeCapture {
        fn prepare_capture(&mut self) -> Result<(), PortError> {
            self.prepared += 1;
            self.failure.map_or(Ok(()), Err)
        }
        fn activate_capture(&mut self) -> Result<(), PortError> {
            self.activated += 1;
            self.failure.map_or(Ok(()), Err)
        }
        fn request_local_restore(&mut self) {
            self.restored += 1;
        }
    }

    #[derive(Default)]
    pub struct FakeFocus {
        pub attempts: usize,
        pub failure: Option<PortError>,
    }
    impl FocusPort for FakeFocus {
        fn prepare_focus(&mut self) -> Result<(), PortError> {
            self.attempts += 1;
            self.failure.map_or(Ok(()), Err)
        }
    }

    #[derive(Default)]
    pub struct FakeInjector {
        pub events: Vec<InputCommand>,
        pub permission_denied: bool,
        pub submission_failed: bool,
    }
    impl InjectorPort for FakeInjector {
        fn readiness(&self) -> Result<(), PortError> {
            if self.permission_denied {
                Err(PortError::PermissionDenied)
            } else {
                Ok(())
            }
        }
        fn post_event(&mut self, event: InputCommand) -> Result<(), PortError> {
            if self.submission_failed {
                return Err(PortError::SubmissionFailed);
            }
            self.events.push(event);
            Ok(())
        }
    }

    pub struct FakeTransport {
        pub messages: Vec<Vec<u8>>,
        pub byte_budget: usize,
        pub unavailable: bool,
    }
    impl Default for FakeTransport {
        fn default() -> Self {
            Self {
                messages: Vec::new(),
                byte_budget: 16 * 1024,
                unavailable: false,
            }
        }
    }
    impl TransportPort for FakeTransport {
        fn try_send(&mut self, payload: &[u8]) -> Result<(), PortError> {
            if self.unavailable {
                return Err(PortError::Unavailable);
            }
            let used: usize = self.messages.iter().map(Vec::len).sum();
            if payload.len() > self.byte_budget.saturating_sub(used) {
                return Err(PortError::QueueFull);
            }
            self.messages.push(payload.to_vec());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fake::*, *};

    #[test]
    fn denied_or_revoked_permission_never_submits_input() {
        let mut injector = FakeInjector {
            permission_denied: true,
            ..Default::default()
        };
        let event = InputCommand::Key {
            key_code: 0x41,
            down: true,
        };
        assert_eq!(
            submit_ready(&mut injector, event.clone()),
            Err(PortError::PermissionDenied)
        );
        assert!(injector.events.is_empty());
        injector.permission_denied = false;
        assert_eq!(submit_ready(&mut injector, event.clone()), Ok(()));
        injector.permission_denied = true;
        assert!(submit_ready(&mut injector, event).is_err());
        assert_eq!(injector.events.len(), 1);
    }

    #[test]
    fn failed_submission_is_not_recorded_as_applied() {
        let mut injector = FakeInjector {
            submission_failed: true,
            ..Default::default()
        };
        assert_eq!(
            submit_ready(&mut injector, InputCommand::ReleaseAll),
            Err(PortError::SubmissionFailed)
        );
        assert!(injector.events.is_empty());
    }

    #[test]
    fn blocked_transport_does_not_prevent_platform_restore() {
        let mut transport = FakeTransport {
            unavailable: true,
            ..Default::default()
        };
        let mut capture = FakeCapture::default();
        assert_eq!(transport.try_send(b"end"), Err(PortError::Unavailable));
        capture.request_local_restore();
        assert_eq!(capture.restored, 1);
    }

    #[test]
    fn fake_transport_enforces_a_byte_budget_before_copying() {
        let mut transport = FakeTransport {
            byte_budget: 4,
            ..Default::default()
        };
        assert_eq!(transport.try_send(b"1234"), Ok(()));
        assert_eq!(transport.try_send(b"5"), Err(PortError::QueueFull));
        assert_eq!(transport.messages, vec![b"1234".to_vec()]);
    }

    #[test]
    fn clock_and_capture_failures_are_controllable_without_sleeping() {
        let clock = FakeClock::default();
        clock.0.set(2500);
        assert_eq!(clock.now_ms(), 2500);
        let mut capture = FakeCapture {
            failure: Some(PortError::Unavailable),
            ..Default::default()
        };
        assert!(capture.prepare_capture().is_err());
        assert!(capture.activate_capture().is_err());
        assert_eq!((capture.prepared, capture.activated), (1, 1));
        let mut focus = FakeFocus {
            failure: Some(PortError::PermissionDenied),
            ..Default::default()
        };
        assert!(focus.prepare_focus().is_err());
        assert_eq!(focus.attempts, 1);
    }
}
