use crate::{
    control_ports::{CapturePort, FocusPort},
    protocol_v2::{
        BootId, ControlFrame, ControllerHandshake, CriticalEvent, CriticalFrame, ProtocolError,
        SessionId,
    },
    routing::{LocalOverride, ReturnReason, RouteEffect, RouteError, RouteState, Router},
};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControllerAction {
    SendControl(ControlFrame),
    OpenInput(SessionId),
    CloseInput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControllerRuntimeError {
    Route(RouteError),
    Protocol(ProtocolError),
    NotActive,
}

pub struct ControllerRuntime {
    router: Router,
    handshake: ControllerHandshake,
    pending_request: Option<u64>,
    pending_session: Option<SessionId>,
    input_sequence: u64,
}

impl ControllerRuntime {
    pub fn new(local_boot: BootId, local_peer_id: String) -> Result<Self, ControllerRuntimeError> {
        Ok(Self {
            router: Router::default(),
            handshake: ControllerHandshake::new(local_boot, local_peer_id)
                .map_err(ControllerRuntimeError::Protocol)?,
            pending_request: None,
            pending_session: None,
            input_sequence: 0,
        })
    }

    pub fn local_override(&self) -> Arc<LocalOverride> {
        self.router.local_override()
    }

    pub fn can_begin(&self) -> bool {
        matches!(
            self.router.state(),
            RouteState::LocalDesktop | RouteState::LocalGame
        )
    }

    pub fn set_game_mode(&mut self, enabled: bool) {
        self.router.set_game_mode(enabled);
    }

    pub fn begin<C: CapturePort>(
        &mut self,
        peer_id: &str,
        target_display: String,
        now_ms: u64,
        capture: &mut C,
    ) -> Result<Vec<ControllerAction>, ControllerRuntimeError> {
        let effects = self
            .router
            .go_remote(peer_id, now_ms, capture)
            .map_err(ControllerRuntimeError::Route)?;
        let Some(RouteEffect::Prepare { request, .. }) = effects.first() else {
            return Ok(vec![]);
        };
        match self.handshake.begin(*request, target_display) {
            Ok(frames) => Ok(frames
                .into_iter()
                .map(ControllerAction::SendControl)
                .collect()),
            Err(error) => {
                self.router.go_local(ReturnReason::NetworkFailed, capture);
                Err(ControllerRuntimeError::Protocol(error))
            }
        }
    }

    pub fn handle_control<C: CapturePort>(
        &mut self,
        frame: &ControlFrame,
        now_ms: u64,
        capture: &mut C,
    ) -> Result<Vec<ControllerAction>, ControllerRuntimeError> {
        match frame {
            ControlFrame::Ready { .. } => {
                if let Err(error) = self.handshake.handle(frame) {
                    self.fail_local(capture);
                    return Err(ControllerRuntimeError::Protocol(error));
                }
                let Some((request, session_id)) = self.handshake.prepared_session() else {
                    self.fail_local(capture);
                    return Err(ControllerRuntimeError::Protocol(
                        ProtocolError::InvalidTransition,
                    ));
                };
                self.pending_request = Some(request);
                self.pending_session = Some(session_id);
                if let Err(error) = self.router.ready(request, session_token(session_id)) {
                    self.fail_local(capture);
                    return Err(ControllerRuntimeError::Route(error));
                }
                Ok(vec![])
            }
            ControlFrame::CommitAck { session_id } => {
                let (Some(request), Some(pending)) = (self.pending_request, self.pending_session)
                else {
                    return Err(ControllerRuntimeError::NotActive);
                };
                if pending != *session_id {
                    self.fail_local(capture);
                    return Err(ControllerRuntimeError::Protocol(
                        ProtocolError::WrongSession,
                    ));
                }
                if let Err(error) = self.handshake.handle(frame) {
                    self.fail_local(capture);
                    return Err(ControllerRuntimeError::Protocol(error));
                }
                if let Err(error) =
                    self.router
                        .commit_ack(request, session_token(*session_id), now_ms, capture)
                {
                    let actions = self.end_after_failed_ack(capture);
                    return if actions.is_empty() {
                        Err(ControllerRuntimeError::Route(error))
                    } else {
                        Ok(actions)
                    };
                }
                self.pending_request = None;
                self.input_sequence = 0;
                Ok(vec![ControllerAction::OpenInput(*session_id)])
            }
            ControlFrame::Pong { .. } => {
                if let Err(error) = self.handshake.handle(frame) {
                    self.fail_local(capture);
                    return Err(ControllerRuntimeError::Protocol(error));
                }
                Ok(vec![])
            }
            ControlFrame::Reject { .. } => {
                let _ = self.handshake.handle(frame);
                self.fail_local(capture);
                Err(ControllerRuntimeError::Protocol(
                    ProtocolError::InvalidTransition,
                ))
            }
            _ => {
                self.fail_local(capture);
                Err(ControllerRuntimeError::Protocol(
                    ProtocolError::InvalidTransition,
                ))
            }
        }
    }

    pub fn advance<C: CapturePort, F: FocusPort>(
        &mut self,
        now_ms: u64,
        keys_released: bool,
        capture: &mut C,
        focus: &mut F,
    ) -> Result<Vec<ControllerAction>, ControllerRuntimeError> {
        let effects = self
            .router
            .advance(now_ms, keys_released, capture, focus)
            .map_err(ControllerRuntimeError::Route)?;
        self.route_effects(effects)
    }

    pub fn go_local<C: CapturePort>(
        &mut self,
        reason: ReturnReason,
        capture: &mut C,
    ) -> Result<Vec<ControllerAction>, ControllerRuntimeError> {
        let effects = self.router.go_local(reason, capture);
        self.route_effects(effects)
    }

    pub fn next_input(
        &mut self,
        event: CriticalEvent,
    ) -> Result<CriticalFrame, ControllerRuntimeError> {
        let session_id = self
            .handshake
            .active_session()
            .ok_or(ControllerRuntimeError::NotActive)?;
        self.input_sequence =
            self.input_sequence
                .checked_add(1)
                .ok_or(ControllerRuntimeError::Protocol(
                    ProtocolError::InvalidField("sequence"),
                ))?;
        Ok(CriticalFrame {
            session_id,
            sequence: self.input_sequence,
            event,
        })
    }

    pub fn ping(&mut self) -> Result<ControllerAction, ControllerRuntimeError> {
        self.handshake
            .ping()
            .map(ControllerAction::SendControl)
            .map_err(ControllerRuntimeError::Protocol)
    }

    fn route_effects(
        &mut self,
        effects: Vec<RouteEffect>,
    ) -> Result<Vec<ControllerAction>, ControllerRuntimeError> {
        let mut actions = Vec::new();
        for effect in effects {
            match effect {
                RouteEffect::Commit { request, session } => {
                    if self.pending_request != Some(request) {
                        return Err(ControllerRuntimeError::NotActive);
                    }
                    let pending = self
                        .pending_session
                        .filter(|pending| session_token(*pending) == session)
                        .ok_or(ControllerRuntimeError::NotActive)?;
                    actions.push(ControllerAction::SendControl(
                        self.handshake
                            .commit(request, pending)
                            .map_err(ControllerRuntimeError::Protocol)?,
                    ));
                }
                RouteEffect::End { reason, .. } => {
                    if self.handshake.active_session().is_some() {
                        actions.push(ControllerAction::SendControl(
                            self.handshake
                                .end(return_reason(reason).into())
                                .map_err(ControllerRuntimeError::Protocol)?,
                        ));
                    } else {
                        self.handshake.abort();
                    }
                    self.pending_request = None;
                    self.pending_session = None;
                    actions.push(ControllerAction::CloseInput);
                }
                RouteEffect::Cancel { .. } => {
                    self.handshake.abort();
                    self.pending_request = None;
                    self.pending_session = None;
                    actions.push(ControllerAction::CloseInput);
                }
                RouteEffect::Prepare { .. } => {}
            }
        }
        Ok(actions)
    }

    fn fail_local<C: CapturePort>(&mut self, capture: &mut C) {
        self.router.go_local(ReturnReason::NetworkFailed, capture);
        self.handshake.abort();
        self.pending_request = None;
        self.pending_session = None;
    }

    fn end_after_failed_ack<C: CapturePort>(&mut self, capture: &mut C) -> Vec<ControllerAction> {
        let mut actions = Vec::new();
        if self.handshake.active_session().is_some() {
            if let Ok(end) = self.handshake.end("late-or-cancelled-ack".into()) {
                actions.push(ControllerAction::SendControl(end));
            }
        }
        self.router.go_local(ReturnReason::Emergency, capture);
        self.pending_request = None;
        self.pending_session = None;
        actions.push(ControllerAction::CloseInput);
        actions
    }
}

fn return_reason(reason: ReturnReason) -> &'static str {
    match reason {
        ReturnReason::User => "user",
        ReturnReason::Emergency => "emergency",
        ReturnReason::Timeout => "timeout",
        ReturnReason::KeysHeld => "keys-held",
        ReturnReason::FocusUnavailable => "focus-unavailable",
        ReturnReason::CaptureFailed => "capture-failed",
        ReturnReason::NetworkFailed => "network-failed",
        ReturnReason::QueueFull => "queue-full",
    }
}

fn session_token(session_id: SessionId) -> u128 {
    u128::from_be_bytes(session_id.nonce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control_ports::fake::{FakeCapture, FakeFocus},
        protocol_v2::CriticalButton,
    };

    fn boot(value: u8) -> BootId {
        BootId([value; 16])
    }

    fn prepared() -> (ControllerRuntime, FakeCapture, u64) {
        let mut runtime = ControllerRuntime::new(boot(1), "windows".into()).unwrap();
        let mut capture = FakeCapture::default();
        let actions = runtime
            .begin("mac", "mac-main".into(), 0, &mut capture)
            .unwrap();
        assert_eq!(actions.len(), 2);
        let ControlFrame::Prepare { request_id, .. } = (match &actions[1] {
            ControllerAction::SendControl(frame) => frame.clone(),
            _ => panic!("prepare"),
        }) else {
            panic!("prepare")
        };
        (runtime, capture, request_id)
    }

    #[test]
    fn router_holds_commit_until_keys_release_and_focus_succeeds() {
        let (mut runtime, mut capture, request) = prepared();
        runtime
            .handle_control(
                &ControlFrame::Ready {
                    request_id: request,
                    receiver_boot: boot(2),
                    input_ready: true,
                },
                100,
                &mut capture,
            )
            .unwrap();
        let mut focus = FakeFocus::default();
        assert!(runtime
            .advance(101, false, &mut capture, &mut focus)
            .unwrap()
            .is_empty());
        let commit = runtime
            .advance(102, true, &mut capture, &mut focus)
            .unwrap();
        let ControllerAction::SendControl(ControlFrame::Commit { session_id, .. }) = commit[0]
        else {
            panic!("commit")
        };
        assert_eq!((focus.attempts, capture.activated), (1, 0));
        assert_eq!(
            runtime
                .handle_control(&ControlFrame::CommitAck { session_id }, 103, &mut capture,)
                .unwrap(),
            vec![ControllerAction::OpenInput(session_id)]
        );
        assert_eq!(capture.activated, 1);
        let frame = runtime
            .next_input(CriticalEvent::Button {
                button: CriticalButton::Left,
                down: true,
                x: 10,
                y: 20,
                motion_sequence: 1,
            })
            .unwrap();
        assert_eq!(frame.sequence, 1);
    }

    #[test]
    fn emergency_before_ack_restores_local_and_never_opens_input() {
        let (mut runtime, mut capture, request) = prepared();
        runtime
            .handle_control(
                &ControlFrame::Ready {
                    request_id: request,
                    receiver_boot: boot(2),
                    input_ready: true,
                },
                1,
                &mut capture,
            )
            .unwrap();
        let mut focus = FakeFocus::default();
        let commit = runtime.advance(2, true, &mut capture, &mut focus).unwrap();
        let ControllerAction::SendControl(ControlFrame::Commit { session_id, .. }) = commit[0]
        else {
            panic!("commit")
        };
        runtime.local_override().request_local();
        let actions = runtime
            .handle_control(&ControlFrame::CommitAck { session_id }, 3, &mut capture)
            .unwrap();
        assert!(!actions
            .iter()
            .any(|action| matches!(action, ControllerAction::OpenInput(_))));
        assert!(actions
            .iter()
            .any(|action| matches!(action, ControllerAction::CloseInput)));
        assert!(runtime
            .next_input(CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            })
            .is_err());
        assert!(capture.restored > 0);
    }

    #[test]
    fn commit_ack_before_router_commit_fails_closed() {
        let (mut runtime, mut capture, request) = prepared();
        runtime
            .handle_control(
                &ControlFrame::Ready {
                    request_id: request,
                    receiver_boot: boot(2),
                    input_ready: true,
                },
                1,
                &mut capture,
            )
            .unwrap();
        let (_, session_id) = runtime.handshake.prepared_session().unwrap();
        assert_eq!(
            runtime.handle_control(&ControlFrame::CommitAck { session_id }, 2, &mut capture,),
            Err(ControllerRuntimeError::Protocol(
                ProtocolError::InvalidTransition
            ))
        );
        assert!(runtime.local_override().is_local());
        assert!(runtime
            .next_input(CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            })
            .is_err());
    }

    #[test]
    fn explicit_return_restores_before_end_is_delivered() {
        let (mut runtime, mut capture, request) = prepared();
        runtime
            .handle_control(
                &ControlFrame::Ready {
                    request_id: request,
                    receiver_boot: boot(2),
                    input_ready: true,
                },
                1,
                &mut capture,
            )
            .unwrap();
        let mut focus = FakeFocus::default();
        let commit = runtime.advance(2, true, &mut capture, &mut focus).unwrap();
        let ControllerAction::SendControl(ControlFrame::Commit { session_id, .. }) = commit[0]
        else {
            panic!("commit")
        };
        runtime
            .handle_control(&ControlFrame::CommitAck { session_id }, 3, &mut capture)
            .unwrap();
        let actions = runtime.go_local(ReturnReason::User, &mut capture).unwrap();
        assert_eq!(capture.restored, 1);
        assert!(matches!(
            actions.first(),
            Some(ControllerAction::SendControl(
                ControlFrame::EndSession { .. }
            ))
        ));
        assert_eq!(actions.last(), Some(&ControllerAction::CloseInput));
    }

    #[test]
    fn invalid_active_control_frame_fails_closed() {
        let (mut runtime, mut capture, request) = prepared();
        runtime
            .handle_control(
                &ControlFrame::Ready {
                    request_id: request,
                    receiver_boot: boot(2),
                    input_ready: true,
                },
                1,
                &mut capture,
            )
            .unwrap();
        let mut focus = FakeFocus::default();
        let commit = runtime.advance(2, true, &mut capture, &mut focus).unwrap();
        let ControllerAction::SendControl(ControlFrame::Commit { session_id, .. }) = commit[0]
        else {
            panic!("commit")
        };
        runtime
            .handle_control(&ControlFrame::CommitAck { session_id }, 3, &mut capture)
            .unwrap();
        assert!(runtime
            .handle_control(
                &ControlFrame::Pong {
                    session_id,
                    sequence: 1,
                    highest_applied_sequence: 0,
                },
                4,
                &mut capture,
            )
            .is_err());
        assert!(runtime.local_override().is_local());
        assert!(runtime
            .next_input(CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            })
            .is_err());
    }
}
