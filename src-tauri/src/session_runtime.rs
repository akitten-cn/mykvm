use crate::{
    control_ports::{submit_ready, InjectorPort, PortError},
    pressed_state::PressedState,
    protocol_v2::{
        BootId, ControlFrame, CriticalFrame, DeviceRole, InputSessionGate, ProtocolError,
        ReceiverHandshake, SessionId,
    },
    quic_transport::{AuthenticatedPeer, PeerRole},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionRuntimeError {
    WrongRole,
    WrongConnection,
    Protocol(ProtocolError),
    Injector(PortError),
}

pub const INPUT_LEASE_TIMEOUT_MS: u64 = 3_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionFault {
    Injector(PortError),
    ReleaseFailed(PortError),
    LeaseExpired,
    InputStreamClosed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionHealth {
    pub active: bool,
    pub highest_applied_sequence: u64,
    pub last_fault: Option<SessionFault>,
}

pub fn receiver_mode_enabled(machine_role: &str, input_mode: &str) -> bool {
    machine_role == "client" && input_mode == "receive"
}

pub struct ReceiverSessionRuntime<I> {
    local_boot: BootId,
    binding: Option<AuthenticatedPeer>,
    handshake: Option<ReceiverHandshake>,
    input_gate: InputSessionGate,
    active_session: Option<SessionId>,
    highest_applied_sequence: u64,
    last_activity_ms: Option<u64>,
    last_fault: Option<SessionFault>,
    pressed: PressedState,
    injector: I,
}

impl<I: InjectorPort> ReceiverSessionRuntime<I> {
    pub fn new(local_boot: BootId, injector: I) -> Self {
        Self {
            local_boot,
            binding: None,
            handshake: None,
            input_gate: InputSessionGate::new(local_boot),
            active_session: None,
            highest_applied_sequence: 0,
            last_activity_ms: None,
            last_fault: None,
            pressed: PressedState::default(),
            injector,
        }
    }

    pub fn handle_control(
        &mut self,
        frame: &ControlFrame,
        peer: &AuthenticatedPeer,
    ) -> Result<Option<ControlFrame>, SessionRuntimeError> {
        self.handle_control_at(frame, peer, 0)
    }

    pub fn handle_control_at(
        &mut self,
        frame: &ControlFrame,
        peer: &AuthenticatedPeer,
        now_ms: u64,
    ) -> Result<Option<ControlFrame>, SessionRuntimeError> {
        if peer.role != PeerRole::Controller {
            return Err(SessionRuntimeError::WrongRole);
        }

        if let ControlFrame::Hello {
            peer_id,
            role: DeviceRole::Controller,
            ..
        } = frame
        {
            if peer_id != &peer.peer_id {
                return Err(SessionRuntimeError::WrongConnection);
            }
            if self.active_session.is_some() {
                return Err(SessionRuntimeError::WrongConnection);
            }
            self.release_pressed()?;
            let mut handshake = ReceiverHandshake::new(peer.peer_id.clone(), self.local_boot)
                .map_err(SessionRuntimeError::Protocol)?;
            let response = handshake
                .handle(frame)
                .map_err(SessionRuntimeError::Protocol)?;
            self.binding = Some(peer.clone());
            self.handshake = Some(handshake);
            self.highest_applied_sequence = 0;
            self.last_activity_ms = None;
            return Ok(response);
        }

        if self.binding.as_ref() != Some(peer) {
            return Err(SessionRuntimeError::WrongConnection);
        }
        if matches!(frame, ControlFrame::Prepare { .. }) {
            self.injector
                .readiness()
                .map_err(SessionRuntimeError::Injector)?;
        }

        let handshake = self
            .handshake
            .as_mut()
            .ok_or(SessionRuntimeError::WrongConnection)?;
        let mut response = handshake
            .handle(frame)
            .map_err(SessionRuntimeError::Protocol)?;

        if let Some(ControlFrame::CommitAck { session_id }) = response.as_ref() {
            if self.active_session.is_none() {
                if !self.pressed.is_empty() {
                    return Err(SessionRuntimeError::Protocol(
                        ProtocolError::InvalidTransition,
                    ));
                }
                self.input_gate
                    .activate(*session_id)
                    .map_err(SessionRuntimeError::Protocol)?;
                self.active_session = Some(*session_id);
                self.highest_applied_sequence = 0;
                self.last_activity_ms = Some(now_ms);
                self.last_fault = None;
            } else if self.active_session != Some(*session_id) {
                return Err(SessionRuntimeError::Protocol(ProtocolError::WrongSession));
            }
        }

        if let ControlFrame::EndSession { session_id, .. } = frame {
            if self.active_session == Some(*session_id) {
                self.input_gate
                    .end(*session_id)
                    .map_err(SessionRuntimeError::Protocol)?;
                self.active_session = None;
                self.last_activity_ms = None;
            }
            self.release_pressed()?;
        }

        if let Some(ControlFrame::Pong {
            highest_applied_sequence,
            ..
        }) = response.as_mut()
        {
            *highest_applied_sequence = self.highest_applied_sequence;
        }
        if self.active_session.is_some()
            && matches!(
                frame,
                ControlFrame::Ping { .. } | ControlFrame::Commit { .. }
            )
        {
            self.last_activity_ms = Some(now_ms);
        }
        Ok(response)
    }

    pub fn handle_input(
        &mut self,
        frame: &CriticalFrame,
        peer: &AuthenticatedPeer,
    ) -> Result<(), SessionRuntimeError> {
        self.handle_input_at(frame, peer, 0)
    }

    pub fn handle_input_at(
        &mut self,
        frame: &CriticalFrame,
        peer: &AuthenticatedPeer,
        now_ms: u64,
    ) -> Result<(), SessionRuntimeError> {
        if self.binding.as_ref() != Some(peer) || peer.role != PeerRole::Controller {
            return Err(SessionRuntimeError::WrongConnection);
        }
        self.input_gate
            .accept(frame)
            .map_err(SessionRuntimeError::Protocol)?;
        let pressed_before = self.pressed.clone();
        for command in self.pressed.apply(&frame.event) {
            if let Err(error) = submit_ready(&mut self.injector, command) {
                if matches!(
                    frame.event,
                    crate::protocol_v2::CriticalEvent::Key { down: false, .. }
                        | crate::protocol_v2::CriticalEvent::Button { down: false, .. }
                ) {
                    self.pressed = pressed_before;
                }
                self.last_fault = Some(SessionFault::Injector(error));
                self.abort_after_injector_failure(frame.session_id);
                return Err(SessionRuntimeError::Injector(error));
            }
        }
        self.highest_applied_sequence = frame.sequence;
        self.last_activity_ms = Some(now_ms);
        Ok(())
    }

    pub fn expire_if_needed(&mut self, now_ms: u64) -> Result<bool, SessionRuntimeError> {
        let (Some(session_id), Some(last_activity_ms)) =
            (self.active_session, self.last_activity_ms)
        else {
            return Ok(false);
        };
        if now_ms.saturating_sub(last_activity_ms) < INPUT_LEASE_TIMEOUT_MS {
            return Ok(false);
        }
        let _ = self.input_gate.end(session_id);
        if let Some(handshake) = self.handshake.as_mut() {
            let _ = handshake.abort_active(session_id);
        }
        self.active_session = None;
        self.last_activity_ms = None;
        self.last_fault = Some(SessionFault::LeaseExpired);
        self.release_pressed()?;
        Ok(true)
    }

    pub fn input_stream_closed(
        &mut self,
        peer: &AuthenticatedPeer,
    ) -> Result<bool, SessionRuntimeError> {
        if self.binding.as_ref() != Some(peer) {
            return Ok(false);
        }
        let Some(session_id) = self.active_session else {
            return Ok(false);
        };
        let _ = self.input_gate.end(session_id);
        if let Some(handshake) = self.handshake.as_mut() {
            let _ = handshake.abort_active(session_id);
        }
        self.active_session = None;
        self.last_activity_ms = None;
        self.last_fault = Some(SessionFault::InputStreamClosed);
        self.release_pressed()?;
        Ok(true)
    }

    pub fn health(&self) -> SessionHealth {
        SessionHealth {
            active: self.active_session.is_some(),
            highest_applied_sequence: self.highest_applied_sequence,
            last_fault: self.last_fault.clone(),
        }
    }

    fn abort_after_injector_failure(&mut self, session_id: SessionId) {
        let _ = self.input_gate.end(session_id);
        if let Some(handshake) = self.handshake.as_mut() {
            let _ = handshake.abort_active(session_id);
        }
        self.active_session = None;
        self.last_activity_ms = None;
        let _ = self.release_pressed();
    }

    fn release_pressed(&mut self) -> Result<(), SessionRuntimeError> {
        let pressed_before = self.pressed.clone();
        let commands = self.pressed.release_all();
        let mut first_error = None;
        for command in commands {
            if let Err(error) = submit_ready(&mut self.injector, command) {
                self.last_fault = Some(SessionFault::ReleaseFailed(error));
                first_error.get_or_insert(SessionRuntimeError::Injector(error));
            }
        }
        if let Some(error) = first_error {
            self.pressed = pressed_before;
            Err(error)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    fn injector(&self) -> &I {
        &self.injector
    }

    #[cfg(test)]
    fn injector_mut(&mut self) -> &mut I {
        &mut self.injector
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_ports::fake::FakeInjector;
    use crate::protocol_v2::{CriticalButton, CriticalEvent};
    use crate::shared_input::{InputCommand, MouseButton};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn boot(value: u8) -> BootId {
        BootId([value; 16])
    }

    #[test]
    fn receiver_mode_requires_the_mac_receive_role() {
        assert!(receiver_mode_enabled("client", "receive"));
        assert!(!receiver_mode_enabled("server", "receive"));
        assert!(!receiver_mode_enabled("client", "control"));
        assert!(!receiver_mode_enabled("unset", "receive"));
    }

    fn peer(generation: u64) -> AuthenticatedPeer {
        AuthenticatedPeer {
            peer_id: "windows-controller".into(),
            role: PeerRole::Controller,
            trust_revision: 3,
            connection_generation: generation,
            remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 47000),
        }
    }

    fn session() -> SessionId {
        SessionId {
            controller_boot: boot(1),
            receiver_boot: boot(2),
            nonce: [3; 16],
        }
    }

    fn activate(
        runtime: &mut ReceiverSessionRuntime<FakeInjector>,
        authenticated: &AuthenticatedPeer,
    ) {
        runtime
            .handle_control(
                &ControlFrame::Hello {
                    boot_id: boot(1),
                    peer_id: authenticated.peer_id.clone(),
                    role: DeviceRole::Controller,
                    capabilities: vec!["control_v2".into(), "input_v2".into()],
                },
                authenticated,
            )
            .unwrap();
        runtime
            .handle_control(
                &ControlFrame::Prepare {
                    request_id: 7,
                    target_display: "mac-main".into(),
                },
                authenticated,
            )
            .unwrap();
        assert_eq!(
            runtime
                .handle_control(
                    &ControlFrame::Commit {
                        request_id: 7,
                        session_id: session(),
                    },
                    authenticated,
                )
                .unwrap(),
            Some(ControlFrame::CommitAck {
                session_id: session()
            })
        );
    }

    #[test]
    fn authenticated_session_dispatches_ordered_input_and_reports_progress() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        runtime
            .handle_input(
                &CriticalFrame {
                    session_id: session(),
                    sequence: 1,
                    event: CriticalEvent::Button {
                        button: CriticalButton::Left,
                        down: true,
                        x: 320,
                        y: 240,
                        motion_sequence: 1,
                    },
                },
                &authenticated,
            )
            .unwrap();
        assert_eq!(
            runtime.injector().events,
            vec![InputCommand::MouseButton {
                button: MouseButton::Left,
                down: true,
                x: 320,
                y: 240,
            }]
        );
        assert_eq!(
            runtime
                .handle_control(
                    &ControlFrame::Ping {
                        session_id: session(),
                        sequence: 9,
                    },
                    &authenticated,
                )
                .unwrap(),
            Some(ControlFrame::Pong {
                session_id: session(),
                sequence: 9,
                highest_applied_sequence: 1,
            })
        );
    }

    #[test]
    fn connection_generation_is_bound_across_control_and_input() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        let frame = CriticalFrame {
            session_id: session(),
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        assert_eq!(
            runtime.handle_input(&frame, &peer(11)),
            Err(SessionRuntimeError::WrongConnection)
        );
        assert!(runtime.injector().events.is_empty());
    }

    #[test]
    fn duplicate_commit_ack_does_not_reset_input_sequence() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        let first = CriticalFrame {
            session_id: session(),
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        runtime.handle_input(&first, &authenticated).unwrap();
        assert_eq!(
            runtime
                .handle_control(
                    &ControlFrame::Commit {
                        request_id: 7,
                        session_id: session(),
                    },
                    &authenticated,
                )
                .unwrap(),
            Some(ControlFrame::CommitAck {
                session_id: session()
            })
        );
        assert!(runtime.handle_input(&first, &authenticated).is_err());
        assert_eq!(runtime.injector().events.len(), 1);
    }

    #[test]
    fn end_releases_and_late_input_stays_rejected() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        runtime
            .handle_input(
                &CriticalFrame {
                    session_id: session(),
                    sequence: 1,
                    event: CriticalEvent::Key {
                        key_code: 65,
                        scan_code: 30,
                        extended: false,
                        down: true,
                    },
                },
                &authenticated,
            )
            .unwrap();
        runtime
            .handle_control(
                &ControlFrame::EndSession {
                    session_id: session(),
                    reason: "return-windows".into(),
                },
                &authenticated,
            )
            .unwrap();
        assert_eq!(
            runtime.injector().events,
            vec![
                InputCommand::Key {
                    key_code: 65,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 65,
                    down: false,
                },
            ]
        );
        let late = CriticalFrame {
            session_id: session(),
            sequence: 2,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: false,
            },
        };
        assert!(runtime.handle_input(&late, &authenticated).is_err());
        assert_eq!(runtime.injector().events.len(), 2);
    }

    #[test]
    fn injector_failure_aborts_session_and_attempts_release() {
        let authenticated = peer(10);
        let injector = FakeInjector {
            submission_failed: true,
            ..Default::default()
        };
        let mut runtime = ReceiverSessionRuntime::new(boot(2), injector);
        activate(&mut runtime, &authenticated);
        let frame = CriticalFrame {
            session_id: session(),
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        assert_eq!(
            runtime.handle_input(&frame, &authenticated),
            Err(SessionRuntimeError::Injector(PortError::SubmissionFailed))
        );
        assert!(runtime.injector().events.is_empty());
        assert!(runtime.handle_input(&frame, &authenticated).is_err());
    }

    #[test]
    fn failed_key_up_remains_in_ledger_and_end_retries_release() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        let down = CriticalFrame {
            session_id: session(),
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        runtime.handle_input(&down, &authenticated).unwrap();
        runtime.injector_mut().submission_failed = true;
        let up = CriticalFrame {
            session_id: session(),
            sequence: 2,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: false,
            },
        };
        assert!(runtime.handle_input(&up, &authenticated).is_err());
        runtime.injector_mut().submission_failed = false;
        runtime
            .handle_control(
                &ControlFrame::EndSession {
                    session_id: session(),
                    reason: "retry-release".into(),
                },
                &authenticated,
            )
            .unwrap();
        assert_eq!(
            runtime.injector().events,
            vec![
                InputCommand::Key {
                    key_code: 65,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 65,
                    down: false,
                },
            ]
        );
    }

    #[test]
    fn readiness_failure_prevents_ready() {
        let authenticated = peer(10);
        let injector = FakeInjector {
            permission_denied: true,
            ..Default::default()
        };
        let mut runtime = ReceiverSessionRuntime::new(boot(2), injector);
        runtime
            .handle_control(
                &ControlFrame::Hello {
                    boot_id: boot(1),
                    peer_id: authenticated.peer_id.clone(),
                    role: DeviceRole::Controller,
                    capabilities: vec!["control_v2".into(), "input_v2".into()],
                },
                &authenticated,
            )
            .unwrap();
        assert_eq!(
            runtime.handle_control(
                &ControlFrame::Prepare {
                    request_id: 7,
                    target_display: "mac-main".into(),
                },
                &authenticated,
            ),
            Err(SessionRuntimeError::Injector(PortError::PermissionDenied))
        );
    }

    #[test]
    fn a18_lease_timeout_releases_pressed_input() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        runtime
            .handle_input_at(
                &CriticalFrame {
                    session_id: session(),
                    sequence: 1,
                    event: CriticalEvent::Key {
                        key_code: 65,
                        scan_code: 30,
                        extended: false,
                        down: true,
                    },
                },
                &authenticated,
                100,
            )
            .unwrap();
        assert!(!runtime.expire_if_needed(3_099).unwrap());
        assert!(runtime.expire_if_needed(3_100).unwrap());
        assert_eq!(
            runtime.injector().events,
            vec![
                InputCommand::Key {
                    key_code: 65,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 65,
                    down: false,
                },
            ]
        );
        assert_eq!(
            runtime.health(),
            SessionHealth {
                active: false,
                highest_applied_sequence: 1,
                last_fault: Some(SessionFault::LeaseExpired),
            }
        );
    }

    #[test]
    fn ping_refreshes_lease_and_release_failure_is_visible() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        runtime
            .handle_input_at(
                &CriticalFrame {
                    session_id: session(),
                    sequence: 1,
                    event: CriticalEvent::Key {
                        key_code: 65,
                        scan_code: 30,
                        extended: false,
                        down: true,
                    },
                },
                &authenticated,
                100,
            )
            .unwrap();
        runtime
            .handle_control_at(
                &ControlFrame::Ping {
                    session_id: session(),
                    sequence: 1,
                },
                &authenticated,
                2_500,
            )
            .unwrap();
        assert!(!runtime.expire_if_needed(5_499).unwrap());
        runtime.injector_mut().submission_failed = true;
        assert_eq!(
            runtime.expire_if_needed(5_500),
            Err(SessionRuntimeError::Injector(PortError::SubmissionFailed))
        );
        assert_eq!(
            runtime.health().last_fault,
            Some(SessionFault::ReleaseFailed(PortError::SubmissionFailed))
        );
    }

    #[test]
    fn input_stream_close_ends_session_and_releases_immediately() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        runtime
            .handle_input(
                &CriticalFrame {
                    session_id: session(),
                    sequence: 1,
                    event: CriticalEvent::Key {
                        key_code: 65,
                        scan_code: 30,
                        extended: false,
                        down: true,
                    },
                },
                &authenticated,
            )
            .unwrap();
        assert!(runtime.input_stream_closed(&authenticated).unwrap());
        assert_eq!(runtime.injector().events.len(), 2);
        assert_eq!(
            runtime.health().last_fault,
            Some(SessionFault::InputStreamClosed)
        );
        assert!(!runtime.input_stream_closed(&authenticated).unwrap());
    }
}
