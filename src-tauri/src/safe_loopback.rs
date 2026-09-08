//! Test-only, two-peer QUIC loopback. It never constructs native capture or
//! injection adapters; all applied input is retained by `FakeInjector`.

use std::{
    fs,
    net::UdpSocket,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    control_ports::fake::FakeInjector,
    protocol_v2::{
        self, BootId, ControlFrame, CriticalEvent, CriticalFrame, DeviceRole, MotionFrame,
        SessionId,
    },
    quic_transport::{
        self, PeerRole, TransportHandle, TrustedPeer, TrustedPeerRegistry, PROTOCOL_VERSION,
    },
    session_runtime::{ReceiverSessionRuntime, SessionRuntimeError},
    shared_input::InputCommand,
};

struct LoopbackEndpoints {
    root: PathBuf,
    controller: TransportHandle,
    receiver: TransportHandle,
}

impl Drop for LoopbackEndpoints {
    fn drop(&mut self) {
        self.controller.shutdown();
        self.receiver.shutdown();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn unused_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .expect("bind temporary UDP port")
        .local_addr()
        .expect("temporary UDP address")
        .port()
}

fn unique_root(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "mykvm-safe-loopback-{label}-{}-{suffix}",
        std::process::id()
    ))
}

#[test]
fn m05_authenticated_loopback_applies_reliable_and_motion_only_to_fake_injector() {
    let root = unique_root("input");
    let runtime = Arc::new(Mutex::new(ReceiverSessionRuntime::new(
        BootId([2; 16]),
        FakeInjector::default(),
    )));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let (applied_tx, applied_rx) = mpsc::channel();

    let runtime_for_motion = Arc::clone(&runtime);
    let errors_for_motion = Arc::clone(&errors);
    let motion_tx = applied_tx.clone();
    let runtime_for_control = Arc::clone(&runtime);
    let errors_for_control = Arc::clone(&errors);
    let runtime_for_input = Arc::clone(&runtime);
    let errors_for_input = Arc::clone(&errors);
    let input_tx = applied_tx.clone();
    let runtime_for_close = Arc::clone(&runtime);

    let receiver = quic_transport::start(
        unused_port(),
        root.join("receiver-identity"),
        TrustedPeerRegistry::default(),
        Arc::new(move |payload, peer| {
            match protocol_v2::decode_motion(&payload)
                .map_err(SessionRuntimeError::Protocol)
                .and_then(|frame| {
                    runtime_for_motion
                        .lock()
                        .expect("receiver runtime lock")
                        .handle_motion_at(frame, &peer, 120)
                }) {
                Ok(disposition) => {
                    let _ = motion_tx.send(format!("motion:{disposition:?}"));
                }
                Err(error) => errors_for_motion
                    .lock()
                    .expect("motion error lock")
                    .push(format!("motion: {error:?}")),
            }
        }),
        Arc::new(|_, _| false),
        Arc::new(move |frame, peer| {
            match runtime_for_control
                .lock()
                .expect("receiver runtime lock")
                .handle_control_at(&frame, &peer, 100)
            {
                Ok(response) => response,
                Err(error) => {
                    errors_for_control
                        .lock()
                        .expect("control error lock")
                        .push(format!("control: {error:?}"));
                    None
                }
            }
        }),
        Arc::new(move |frame, peer| {
            match runtime_for_input
                .lock()
                .expect("receiver runtime lock")
                .handle_input_at(&frame, &peer, 110)
            {
                Ok(()) => {
                    let _ = input_tx.send(format!("input:{}", frame.sequence));
                    true
                }
                Err(error) => {
                    errors_for_input
                        .lock()
                        .expect("input error lock")
                        .push(format!("input: {error:?}"));
                    false
                }
            }
        }),
        Arc::new(move |peer, _| {
            let _ = runtime_for_close
                .lock()
                .expect("receiver runtime lock")
                .input_stream_closed(&peer);
        }),
    )
    .expect("start isolated receiver");

    let controller = quic_transport::start(
        unused_port(),
        root.join("controller-identity"),
        TrustedPeerRegistry::default(),
        Arc::new(|_, _| {}),
        Arc::new(|_, _| false),
        Arc::new(|_, _| None),
        Arc::new(|_, _| false),
        Arc::new(|_, _| {}),
    )
    .expect("start isolated controller");

    receiver
        .replace_trusted_peers(vec![TrustedPeer {
            peer_id: "fixture-controller".into(),
            certificate: controller.public_key().into(),
            role: PeerRole::Controller,
            trust_revision: 1,
        }])
        .expect("trust controller fixture certificate");
    controller
        .replace_trusted_peers(vec![TrustedPeer {
            peer_id: "fixture-receiver".into(),
            certificate: receiver.public_key().into(),
            role: PeerRole::Receiver,
            trust_revision: 1,
        }])
        .expect("trust receiver fixture certificate");

    let endpoints = LoopbackEndpoints {
        root,
        controller,
        receiver,
    };
    let receiver_addr = format!("127.0.0.1:{}", endpoints.receiver.port());
    let receiver_peer = || {
        endpoints
            .controller
            .control_peer(
                "fixture-receiver",
                PeerRole::Receiver,
                receiver_addr.clone(),
                PROTOCOL_VERSION,
            )
            .expect("build trusted receiver target")
    };

    let (response_tx, response_rx) = mpsc::channel();
    let control = endpoints
        .controller
        .open_control(
            receiver_peer(),
            Arc::new(move |frame| {
                let _ = response_tx.send(frame);
                None
            }),
        )
        .expect("open authenticated control stream");
    control
        .try_send(ControlFrame::Hello {
            boot_id: BootId([1; 16]),
            peer_id: "fixture-controller".into(),
            role: DeviceRole::Controller,
            capabilities: vec!["control_v2".into(), "input_v2".into()],
        })
        .expect("send hello");
    control
        .try_send(ControlFrame::Prepare {
            request_id: 7,
            target_display: "mac-main".into(),
            layout_revision: 1,
        })
        .expect("send prepare");
    assert!(matches!(
        response_rx.recv_timeout(Duration::from_secs(2)),
        Ok(ControlFrame::Ready {
            request_id: 7,
            input_ready: true,
            ..
        })
    ));

    let session_id = SessionId {
        controller_boot: BootId([1; 16]),
        receiver_boot: BootId([2; 16]),
        nonce: [3; 16],
    };
    control
        .try_send(ControlFrame::Commit {
            request_id: 7,
            session_id,
        })
        .expect("send commit");
    assert_eq!(
        response_rx.recv_timeout(Duration::from_secs(2)),
        Ok(ControlFrame::CommitAck { session_id })
    );

    let input = endpoints
        .controller
        .open_input(receiver_peer())
        .expect("open authenticated input stream");
    input
        .try_send(&CriticalFrame {
            session_id,
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 0x41,
                scan_code: 30,
                extended: false,
                down: true,
            },
        })
        .expect("send reliable key-down");
    assert_eq!(
        applied_rx.recv_timeout(Duration::from_secs(2)).as_deref(),
        Ok("input:1")
    );

    let motion = MotionFrame {
        session_id,
        display_id: "mac-main".into(),
        layout_revision: 1,
        sequence: 2,
        required_reliable_sequence: 1,
        x: 640,
        y: 400,
    };
    endpoints
        .controller
        .send_datagram(
            endpoints.controller.peer(
                receiver_addr,
                endpoints.receiver.public_key().into(),
                PROTOCOL_VERSION,
            ),
            protocol_v2::encode_motion(&motion).expect("encode motion fixture"),
        )
        .expect("send authenticated motion");
    assert_eq!(
        applied_rx.recv_timeout(Duration::from_secs(2)).as_deref(),
        Ok("motion:Applied")
    );

    input
        .try_send(&CriticalFrame {
            session_id,
            sequence: 2,
            event: CriticalEvent::Key {
                key_code: 0x41,
                scan_code: 30,
                extended: false,
                down: false,
            },
        })
        .expect("send reliable key-up");
    assert_eq!(
        applied_rx.recv_timeout(Duration::from_secs(2)).as_deref(),
        Ok("input:2")
    );

    let runtime = runtime.lock().expect("receiver runtime lock");
    assert_eq!(
        runtime.injector().events,
        vec![
            InputCommand::Key {
                key_code: 0x41,
                down: true,
            },
            InputCommand::MouseMove {
                x: 640,
                y: 400,
                drag_button: None,
            },
            InputCommand::Key {
                key_code: 0x41,
                down: false,
            },
        ]
    );
    assert!(errors.lock().expect("error lock").is_empty());
}
