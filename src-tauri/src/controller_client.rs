use crate::{
    control_ports::{CapturePort, FocusPort},
    controller_runtime::{ControllerAction, ControllerRuntime, ControllerRuntimeError},
    protocol_v2::{BootId, ControlFrame, CriticalEvent, CriticalFrame},
    quic_transport::{ControlHandle, ControlPeer, InputHandle, PeerRole, TransportHandle},
    routing::{LocalOverride, ReturnReason},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
    Arc,
};

const INBOUND_CONTROL_FRAMES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerTarget {
    pub peer_id: String,
    pub addr: String,
    pub protocol_version: u16,
    pub target_display: String,
}

#[derive(Clone)]
pub struct ControllerInbox {
    sender: SyncSender<ControlFrame>,
    overflowed: Arc<AtomicBool>,
}

impl ControllerInbox {
    fn offer(&self, frame: ControlFrame) {
        if self.sender.try_send(frame).is_err() {
            self.overflowed.store(true, Ordering::Release);
        }
    }
}

pub trait ControllerTransport {
    fn connect(&mut self, target: &ControllerTarget, inbox: ControllerInbox) -> Result<(), String>;
    fn send_control(&mut self, frame: ControlFrame) -> Result<(), String>;
    fn open_input(&mut self) -> Result<(), String>;
    fn send_input(&mut self, frame: &CriticalFrame) -> Result<(), String>;
    fn disconnect(&mut self);
}

pub struct QuicControllerTransport {
    transport: TransportHandle,
    peer: Option<ControlPeer>,
    control: Option<ControlHandle>,
    input: Option<InputHandle>,
}

impl QuicControllerTransport {
    pub fn new(transport: TransportHandle) -> Self {
        Self {
            transport,
            peer: None,
            control: None,
            input: None,
        }
    }
}

impl ControllerTransport for QuicControllerTransport {
    fn connect(&mut self, target: &ControllerTarget, inbox: ControllerInbox) -> Result<(), String> {
        self.disconnect();
        let peer = self.transport.control_peer(
            &target.peer_id,
            PeerRole::Receiver,
            target.addr.clone(),
            target.protocol_version,
        )?;
        let control = self.transport.open_control(
            peer.clone(),
            Arc::new(move |frame| {
                inbox.offer(frame);
                None
            }),
        )?;
        self.peer = Some(peer);
        self.control = Some(control);
        Ok(())
    }

    fn send_control(&mut self, frame: ControlFrame) -> Result<(), String> {
        self.control
            .as_ref()
            .ok_or_else(|| "V2 control stream is not open".to_string())?
            .try_send(frame)
    }

    fn open_input(&mut self) -> Result<(), String> {
        let peer = self
            .peer
            .clone()
            .ok_or_else(|| "V2 control peer is not connected".to_string())?;
        self.input = Some(self.transport.open_input(peer)?);
        Ok(())
    }

    fn send_input(&mut self, frame: &CriticalFrame) -> Result<(), String> {
        self.input
            .as_ref()
            .ok_or_else(|| "V2 input stream is not open".to_string())?
            .try_send(frame)
    }

    fn disconnect(&mut self) {
        self.input = None;
        self.control = None;
        self.peer = None;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControllerClientError {
    Runtime(ControllerRuntimeError),
    Transport(String),
    InboundOverflow,
}

pub struct ControllerClient<T: ControllerTransport> {
    runtime: ControllerRuntime,
    transport: T,
    inbound: Receiver<ControlFrame>,
    inbox: ControllerInbox,
}

impl<T: ControllerTransport> ControllerClient<T> {
    pub fn new(
        transport: T,
        local_boot: BootId,
        local_peer_id: String,
    ) -> Result<Self, ControllerClientError> {
        let (sender, inbound) = mpsc::sync_channel(INBOUND_CONTROL_FRAMES);
        Ok(Self {
            runtime: ControllerRuntime::new(local_boot, local_peer_id)
                .map_err(ControllerClientError::Runtime)?,
            transport,
            inbound,
            inbox: ControllerInbox {
                sender,
                overflowed: Arc::new(AtomicBool::new(false)),
            },
        })
    }

    pub fn local_override(&self) -> Arc<LocalOverride> {
        self.runtime.local_override()
    }

    pub fn set_game_mode(&mut self, enabled: bool) {
        self.runtime.set_game_mode(enabled);
    }

    pub fn begin<C: CapturePort>(
        &mut self,
        target: &ControllerTarget,
        now_ms: u64,
        capture: &mut C,
    ) -> Result<(), ControllerClientError> {
        if !self.runtime.can_begin() {
            return Err(ControllerClientError::Runtime(
                ControllerRuntimeError::Route(crate::routing::RouteError::Busy),
            ));
        }
        self.clear_inbound();
        self.transport
            .connect(target, self.inbox.clone())
            .map_err(ControllerClientError::Transport)?;
        let actions = match self.runtime.begin(
            &target.peer_id,
            target.target_display.clone(),
            now_ms,
            capture,
        ) {
            Ok(actions) => actions,
            Err(error) => {
                self.transport.disconnect();
                return Err(ControllerClientError::Runtime(error));
            }
        };
        self.apply(actions, capture)
    }

    pub fn poll<C: CapturePort, F: FocusPort>(
        &mut self,
        now_ms: u64,
        keys_released: bool,
        capture: &mut C,
        focus: &mut F,
    ) -> Result<(), ControllerClientError> {
        if self.inbox.overflowed.swap(false, Ordering::AcqRel) {
            self.fail_transport(capture);
            return Err(ControllerClientError::InboundOverflow);
        }
        loop {
            match self.inbound.try_recv() {
                Ok(frame) => {
                    let actions = match self.runtime.handle_control(&frame, now_ms, capture) {
                        Ok(actions) => actions,
                        Err(error) => {
                            self.transport.disconnect();
                            return Err(ControllerClientError::Runtime(error));
                        }
                    };
                    self.apply(actions, capture)?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.fail_transport(capture);
                    return Err(ControllerClientError::Transport(
                        "V2 control inbox disconnected".into(),
                    ));
                }
            }
        }
        let actions = match self.runtime.advance(now_ms, keys_released, capture, focus) {
            Ok(actions) => actions,
            Err(error) => {
                self.fail_transport(capture);
                return Err(ControllerClientError::Runtime(error));
            }
        };
        self.apply(actions, capture)
    }

    pub fn send_input<C: CapturePort>(
        &mut self,
        event: CriticalEvent,
        capture: &mut C,
    ) -> Result<(), ControllerClientError> {
        let frame = self
            .runtime
            .next_input(event)
            .map_err(ControllerClientError::Runtime)?;
        if let Err(error) = self.transport.send_input(&frame) {
            self.fail_transport(capture);
            return Err(ControllerClientError::Transport(error));
        }
        Ok(())
    }

    pub fn go_local<C: CapturePort>(
        &mut self,
        reason: ReturnReason,
        capture: &mut C,
    ) -> Result<(), ControllerClientError> {
        let actions = self
            .runtime
            .go_local(reason, capture)
            .map_err(ControllerClientError::Runtime)?;
        self.apply(actions, capture)
    }

    fn apply<C: CapturePort>(
        &mut self,
        actions: Vec<ControllerAction>,
        capture: &mut C,
    ) -> Result<(), ControllerClientError> {
        for action in actions {
            let result = match action {
                ControllerAction::SendControl(frame) => self.transport.send_control(frame),
                ControllerAction::OpenInput(_) => self.transport.open_input(),
                ControllerAction::CloseInput => {
                    self.transport.disconnect();
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.fail_transport(capture);
                return Err(ControllerClientError::Transport(error));
            }
        }
        Ok(())
    }

    fn fail_transport<C: CapturePort>(&mut self, capture: &mut C) {
        if let Ok(actions) = self.runtime.go_local(ReturnReason::NetworkFailed, capture) {
            for action in actions {
                if let ControllerAction::SendControl(frame) = action {
                    let _ = self.transport.send_control(frame);
                }
            }
        }
        self.transport.disconnect();
    }

    fn clear_inbound(&mut self) {
        self.inbox.overflowed.store(false, Ordering::Release);
        while self.inbound.try_recv().is_ok() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control_ports::fake::{FakeCapture, FakeFocus},
        protocol_v2::{CriticalButton, SessionId},
    };

    #[derive(Default)]
    struct FakeControllerTransport {
        inbox: Option<ControllerInbox>,
        controls: Vec<ControlFrame>,
        inputs: Vec<CriticalFrame>,
        input_open: bool,
        fail_input: bool,
    }

    impl FakeControllerTransport {
        fn receive(&self, frame: ControlFrame) {
            self.inbox.as_ref().unwrap().offer(frame);
        }
    }

    impl ControllerTransport for FakeControllerTransport {
        fn connect(
            &mut self,
            _target: &ControllerTarget,
            inbox: ControllerInbox,
        ) -> Result<(), String> {
            self.inbox = Some(inbox);
            Ok(())
        }

        fn send_control(&mut self, frame: ControlFrame) -> Result<(), String> {
            self.controls.push(frame);
            Ok(())
        }

        fn open_input(&mut self) -> Result<(), String> {
            self.input_open = true;
            Ok(())
        }

        fn send_input(&mut self, frame: &CriticalFrame) -> Result<(), String> {
            if self.fail_input {
                return Err("input closed".into());
            }
            self.inputs.push(frame.clone());
            Ok(())
        }

        fn disconnect(&mut self) {
            self.input_open = false;
        }
    }

    fn boot(value: u8) -> BootId {
        BootId([value; 16])
    }

    fn target() -> ControllerTarget {
        ControllerTarget {
            peer_id: "mac".into(),
            addr: "127.0.0.1:44888".into(),
            protocol_version: 2,
            target_display: "mac-main".into(),
        }
    }

    fn start() -> (ControllerClient<FakeControllerTransport>, FakeCapture, u64) {
        let mut client = ControllerClient::new(
            FakeControllerTransport::default(),
            boot(1),
            "windows".into(),
        )
        .unwrap();
        let mut capture = FakeCapture::default();
        client.begin(&target(), 0, &mut capture).unwrap();
        let ControlFrame::Prepare { request_id, .. } = client.transport.controls[1] else {
            panic!("prepare")
        };
        (client, capture, request_id)
    }

    #[test]
    fn bounded_inbox_drives_commit_then_reliable_input() {
        let (mut client, mut capture, request_id) = start();
        client.transport.receive(ControlFrame::Ready {
            request_id,
            receiver_boot: boot(2),
            input_ready: true,
        });
        let mut focus = FakeFocus::default();
        client.poll(1, false, &mut capture, &mut focus).unwrap();
        assert_eq!(client.transport.controls.len(), 2);
        client.poll(2, true, &mut capture, &mut focus).unwrap();
        let ControlFrame::Commit { session_id, .. } = client.transport.controls[2] else {
            panic!("commit")
        };
        client
            .transport
            .receive(ControlFrame::CommitAck { session_id });
        client.poll(3, true, &mut capture, &mut focus).unwrap();
        assert!(client.transport.input_open);
        client
            .send_input(
                CriticalEvent::Button {
                    button: CriticalButton::Left,
                    down: true,
                    x: 10,
                    y: 20,
                    motion_sequence: 1,
                },
                &mut capture,
            )
            .unwrap();
        assert_eq!(client.transport.inputs[0].sequence, 1);
    }

    #[test]
    fn input_queue_failure_restores_local_and_disconnects() {
        let (mut client, mut capture, request_id) = start();
        let session_id = SessionId {
            controller_boot: boot(1),
            receiver_boot: boot(2),
            nonce: [7; 16],
        };
        client.transport.receive(ControlFrame::Ready {
            request_id,
            receiver_boot: session_id.receiver_boot,
            input_ready: true,
        });
        let mut focus = FakeFocus::default();
        client.poll(1, true, &mut capture, &mut focus).unwrap();
        let ControlFrame::Commit { session_id, .. } = client.transport.controls[2] else {
            panic!("commit")
        };
        client
            .transport
            .receive(ControlFrame::CommitAck { session_id });
        client.poll(2, true, &mut capture, &mut focus).unwrap();
        client.transport.fail_input = true;
        assert!(client
            .send_input(
                CriticalEvent::Key {
                    key_code: 65,
                    scan_code: 30,
                    extended: false,
                    down: true,
                },
                &mut capture,
            )
            .is_err());
        assert!(client.local_override().is_local());
        assert!(!client.transport.input_open);
        assert_eq!(capture.restored, 1);
    }

    #[test]
    fn repeated_begin_does_not_replace_in_flight_connection() {
        let (mut client, mut capture, _) = start();
        assert_eq!(
            client.begin(&target(), 1, &mut capture),
            Err(ControllerClientError::Runtime(
                ControllerRuntimeError::Route(crate::routing::RouteError::Busy)
            ))
        );
        assert_eq!(client.transport.controls.len(), 2);
        assert!(client.transport.inbox.is_some());
    }
}
