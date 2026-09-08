use crate::{
    control_ports::{submit_ready, InjectorPort, PortError},
    protocol_v2::{
        BootId, ControlFrame, CriticalButton, CriticalEvent, CriticalFrame, DeviceRole,
        InputSessionGate, ProtocolError, ReceiverHandshake, SessionId,
    },
    quic_transport::{AuthenticatedPeer, PeerRole},
    shared_input::{InputCommand, MouseButton},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionRuntimeError {
    WrongRole,
    WrongConnection,
    Protocol(ProtocolError),
    Injector(PortError),
}

pub struct ReceiverSessionRuntime<I> {
    local_boot: BootId,
    binding: Option<AuthenticatedPeer>,
    handshake: Option<ReceiverHandshake>,
    input_gate: InputSessionGate,
    active_session: Option<SessionId>,
    highest_applied_sequence: u64,
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
            injector,
        }
    }

    pub fn handle_control(
        &mut self,
        frame: &ControlFrame,
        peer: &AuthenticatedPeer,
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
            let mut handshake = ReceiverHandshake::new(peer.peer_id.clone(), self.local_boot)
                .map_err(SessionRuntimeError::Protocol)?;
            let response = handshake
                .handle(frame)
                .map_err(SessionRuntimeError::Protocol)?;
            self.binding = Some(peer.clone());
            self.handshake = Some(handshake);
            self.highest_applied_sequence = 0;
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
                self.input_gate
                    .activate(*session_id)
                    .map_err(SessionRuntimeError::Protocol)?;
                self.active_session = Some(*session_id);
                self.highest_applied_sequence = 0;
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
                self.injector
                    .post_event(InputCommand::ReleaseAll)
                    .map_err(SessionRuntimeError::Injector)?;
            }
        }

        if let Some(ControlFrame::Pong {
            highest_applied_sequence,
            ..
        }) = response.as_mut()
        {
            *highest_applied_sequence = self.highest_applied_sequence;
        }
        Ok(response)
    }

    pub fn handle_input(
        &mut self,
        frame: &CriticalFrame,
        peer: &AuthenticatedPeer,
    ) -> Result<(), SessionRuntimeError> {
        if self.binding.as_ref() != Some(peer) || peer.role != PeerRole::Controller {
            return Err(SessionRuntimeError::WrongConnection);
        }
        self.input_gate
            .accept(frame)
            .map_err(SessionRuntimeError::Protocol)?;
        let command = critical_command(frame);
        if let Err(error) = submit_ready(&mut self.injector, command) {
            self.abort_after_injector_failure(frame.session_id);
            return Err(SessionRuntimeError::Injector(error));
        }
        self.highest_applied_sequence = frame.sequence;
        Ok(())
    }

    fn abort_after_injector_failure(&mut self, session_id: SessionId) {
        let _ = self.input_gate.end(session_id);
        if let Some(handshake) = self.handshake.as_mut() {
            let _ = handshake.abort_active(session_id);
        }
        self.active_session = None;
        let _ = self.injector.post_event(InputCommand::ReleaseAll);
    }

    #[cfg(test)]
    fn injector(&self) -> &I {
        &self.injector
    }
}

fn critical_command(frame: &CriticalFrame) -> InputCommand {
    match frame.event {
        CriticalEvent::Key { key_code, down, .. } => InputCommand::Key { key_code, down },
        CriticalEvent::Button {
            button, down, x, y, ..
        } => InputCommand::MouseButton {
            button: match button {
                CriticalButton::Left => MouseButton::Left,
                CriticalButton::Right => MouseButton::Right,
                CriticalButton::Middle => MouseButton::Middle,
                CriticalButton::Back => MouseButton::Back,
                CriticalButton::Forward => MouseButton::Forward,
            },
            down,
            x,
            y,
        },
        CriticalEvent::Scroll {
            delta_x, delta_y, ..
        } => InputCommand::Scroll { delta_x, delta_y },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_ports::fake::FakeInjector;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn boot(value: u8) -> BootId {
        BootId([value; 16])
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
            .handle_control(
                &ControlFrame::EndSession {
                    session_id: session(),
                    reason: "return-windows".into(),
                },
                &authenticated,
            )
            .unwrap();
        assert_eq!(runtime.injector().events, vec![InputCommand::ReleaseAll]);
        let late = CriticalFrame {
            session_id: session(),
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: false,
            },
        };
        assert!(runtime.handle_input(&late, &authenticated).is_err());
        assert_eq!(runtime.injector().events, vec![InputCommand::ReleaseAll]);
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
}
