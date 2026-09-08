use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_MAJOR: u16 = 2;
pub const MAX_CONTROL_FRAME_BYTES: usize = 16 * 1024;
const MAGIC: u32 = u32::from_be_bytes(*b"MKV2");
const MAX_PEER_ID_BYTES: usize = 256;
const MAX_CAPABILITIES: usize = 32;
const MAX_CAPABILITY_BYTES: usize = 64;
const MAX_DETAIL_BYTES: usize = 512;
const REQUIRED_CAPABILITIES: [&str; 2] = ["control_v2", "input_v2"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Controller,
    Receiver,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId {
    pub controller_boot: BootId,
    pub receiver_boot: BootId,
    pub nonce: [u8; 16],
}

impl BootId {
    pub fn generate() -> Result<Self, ProtocolError> {
        let mut value = [0_u8; 16];
        SystemRandom::new()
            .fill(&mut value)
            .map_err(|_| ProtocolError::RandomUnavailable)?;
        if value == [0; 16] {
            return Err(ProtocolError::RandomUnavailable);
        }
        Ok(Self(value))
    }
}

impl SessionId {
    pub fn generate(controller_boot: BootId, receiver_boot: BootId) -> Result<Self, ProtocolError> {
        let mut nonce = [0_u8; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| ProtocolError::RandomUnavailable)?;
        if nonce == [0; 16] {
            return Err(ProtocolError::RandomUnavailable);
        }
        Ok(Self {
            controller_boot,
            receiver_boot,
            nonce,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlFrame {
    Hello {
        boot_id: BootId,
        peer_id: String,
        role: DeviceRole,
        capabilities: Vec<String>,
    },
    Prepare {
        request_id: u64,
        target_display: String,
    },
    Ready {
        request_id: u64,
        receiver_boot: BootId,
        input_ready: bool,
    },
    Commit {
        request_id: u64,
        session_id: SessionId,
    },
    CommitAck {
        session_id: SessionId,
    },
    EndSession {
        session_id: SessionId,
        reason: String,
    },
    Ping {
        session_id: SessionId,
        sequence: u64,
    },
    Pong {
        session_id: SessionId,
        sequence: u64,
        highest_applied_sequence: u64,
    },
    Reject {
        code: String,
        detail: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Envelope {
    magic: u32,
    major: u16,
    frame: ControlFrame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    FrameTooLarge(usize),
    InvalidLength,
    Truncated,
    Decode,
    InvalidMagic,
    UnsupportedMajor(u16),
    InvalidField(&'static str),
    InvalidTransition,
    WrongBoot,
    WrongSession,
    WrongRole,
    RandomUnavailable,
}

pub fn encode_control(frame: &ControlFrame) -> Result<Vec<u8>, ProtocolError> {
    validate_frame(frame)?;
    let payload = rmp_serde::to_vec_named(&Envelope {
        magic: MAGIC,
        major: PROTOCOL_MAJOR,
        frame: frame.clone(),
    })
    .map_err(|_| ProtocolError::Decode)?;
    if payload.is_empty() || payload.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(payload.len()));
    }
    let length = u32::try_from(payload.len()).map_err(|_| ProtocolError::InvalidLength)?;
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}

#[derive(Default)]
pub struct ControlDecoder {
    prefix: [u8; 4],
    prefix_len: usize,
    expected: Option<usize>,
    payload: Vec<u8>,
}

impl ControlDecoder {
    pub fn push(&mut self, mut bytes: &[u8]) -> Result<Vec<ControlFrame>, ProtocolError> {
        let mut decoded = Vec::new();
        while !bytes.is_empty() {
            if self.expected.is_none() {
                let take = (4 - self.prefix_len).min(bytes.len());
                self.prefix[self.prefix_len..self.prefix_len + take]
                    .copy_from_slice(&bytes[..take]);
                self.prefix_len += take;
                bytes = &bytes[take..];
                if self.prefix_len < 4 {
                    continue;
                }
                let length = u32::from_be_bytes(self.prefix) as usize;
                if length == 0 {
                    self.reset();
                    return Err(ProtocolError::InvalidLength);
                }
                if length > MAX_CONTROL_FRAME_BYTES {
                    self.reset();
                    return Err(ProtocolError::FrameTooLarge(length));
                }
                self.expected = Some(length);
                self.payload.clear();
                self.payload.reserve(length);
            }

            let expected = self.expected.expect("length set after prefix");
            let take = (expected - self.payload.len()).min(bytes.len());
            self.payload.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.payload.len() == expected {
                decoded.push(decode_payload(&self.payload)?);
                self.reset();
            }
        }
        Ok(decoded)
    }

    pub fn finish(self) -> Result<(), ProtocolError> {
        if self.prefix_len == 0 && self.expected.is_none() {
            Ok(())
        } else {
            Err(ProtocolError::Truncated)
        }
    }

    fn reset(&mut self) {
        self.prefix_len = 0;
        self.expected = None;
        self.payload.clear();
    }
}

fn decode_payload(payload: &[u8]) -> Result<ControlFrame, ProtocolError> {
    let envelope: Envelope = rmp_serde::from_slice(payload).map_err(|_| ProtocolError::Decode)?;
    if envelope.magic != MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    if envelope.major != PROTOCOL_MAJOR {
        return Err(ProtocolError::UnsupportedMajor(envelope.major));
    }
    validate_frame(&envelope.frame)?;
    Ok(envelope.frame)
}

fn validate_frame(frame: &ControlFrame) -> Result<(), ProtocolError> {
    match frame {
        ControlFrame::Hello {
            peer_id,
            capabilities,
            ..
        } => {
            validate_text(peer_id, MAX_PEER_ID_BYTES, "peer_id")?;
            if capabilities.len() > MAX_CAPABILITIES {
                return Err(ProtocolError::InvalidField("capabilities"));
            }
            for capability in capabilities {
                validate_text(capability, MAX_CAPABILITY_BYTES, "capability")?;
            }
            if !REQUIRED_CAPABILITIES
                .iter()
                .all(|required| capabilities.iter().any(|value| value == required))
            {
                return Err(ProtocolError::InvalidField("capabilities"));
            }
        }
        ControlFrame::Prepare {
            request_id,
            target_display,
        } => {
            if *request_id == 0 {
                return Err(ProtocolError::InvalidField("request_id"));
            }
            validate_text(target_display, MAX_PEER_ID_BYTES, "target_display")?;
        }
        ControlFrame::Ready { request_id, .. } if *request_id == 0 => {
            return Err(ProtocolError::InvalidField("request_id"));
        }
        ControlFrame::Commit {
            request_id,
            session_id,
        } => {
            if *request_id == 0 {
                return Err(ProtocolError::InvalidField("request_id"));
            }
            validate_session(session_id)?;
        }
        ControlFrame::CommitAck { session_id }
        | ControlFrame::Ping { session_id, .. }
        | ControlFrame::Pong { session_id, .. } => validate_session(session_id)?,
        ControlFrame::EndSession { session_id, reason } => {
            validate_session(session_id)?;
            validate_text(reason, MAX_DETAIL_BYTES, "reason")?;
        }
        ControlFrame::Reject { code, detail } => {
            validate_text(code, MAX_CAPABILITY_BYTES, "code")?;
            validate_text(detail, MAX_DETAIL_BYTES, "detail")?;
        }
        _ => {}
    }
    Ok(())
}

fn validate_session(session: &SessionId) -> Result<(), ProtocolError> {
    if session.controller_boot.0 == [0; 16]
        || session.receiver_boot.0 == [0; 16]
        || session.nonce == [0; 16]
    {
        Err(ProtocolError::InvalidField("session_id"))
    } else {
        Ok(())
    }
}

fn validate_text(value: &str, max: usize, field: &'static str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() || value.len() > max {
        Err(ProtocolError::InvalidField(field))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiverState {
    AwaitHello,
    Idle {
        controller_boot: BootId,
    },
    Prepared {
        controller_boot: BootId,
        request_id: u64,
    },
    Active {
        session_id: SessionId,
    },
    Ended {
        session_id: SessionId,
    },
}

pub struct ReceiverHandshake {
    trusted_peer_id: String,
    local_boot: BootId,
    state: ReceiverState,
}

impl ReceiverHandshake {
    pub fn new(trusted_peer_id: String, local_boot: BootId) -> Result<Self, ProtocolError> {
        validate_text(&trusted_peer_id, MAX_PEER_ID_BYTES, "peer_id")?;
        Ok(Self {
            trusted_peer_id,
            local_boot,
            state: ReceiverState::AwaitHello,
        })
    }

    pub fn handle(&mut self, frame: &ControlFrame) -> Result<Option<ControlFrame>, ProtocolError> {
        validate_frame(frame)?;
        match (self.state, frame) {
            (
                ReceiverState::AwaitHello,
                ControlFrame::Hello {
                    boot_id,
                    peer_id,
                    role: DeviceRole::Controller,
                    ..
                },
            ) if peer_id == &self.trusted_peer_id => {
                self.state = ReceiverState::Idle {
                    controller_boot: *boot_id,
                };
                Ok(None)
            }
            (ReceiverState::AwaitHello, ControlFrame::Hello { .. }) => {
                Err(ProtocolError::WrongRole)
            }
            (
                ReceiverState::Idle { controller_boot },
                ControlFrame::Prepare {
                    request_id,
                    target_display: _,
                },
            ) => {
                self.state = ReceiverState::Prepared {
                    controller_boot,
                    request_id: *request_id,
                };
                Ok(Some(ControlFrame::Ready {
                    request_id: *request_id,
                    receiver_boot: self.local_boot,
                    input_ready: true,
                }))
            }
            (
                ReceiverState::Prepared {
                    controller_boot,
                    request_id,
                },
                ControlFrame::Commit {
                    request_id: incoming_request,
                    session_id,
                },
            ) => {
                if *incoming_request != request_id {
                    return Err(ProtocolError::InvalidTransition);
                }
                if session_id.controller_boot != controller_boot
                    || session_id.receiver_boot != self.local_boot
                {
                    return Err(ProtocolError::WrongBoot);
                }
                self.state = ReceiverState::Active {
                    session_id: *session_id,
                };
                Ok(Some(ControlFrame::CommitAck {
                    session_id: *session_id,
                }))
            }
            (
                ReceiverState::Active { session_id },
                ControlFrame::Commit {
                    session_id: next, ..
                },
            ) if session_id == *next => Ok(Some(ControlFrame::CommitAck { session_id })),
            (
                ReceiverState::Active { session_id },
                ControlFrame::EndSession {
                    session_id: end, ..
                },
            ) if session_id == *end => {
                self.state = ReceiverState::Ended { session_id };
                Ok(None)
            }
            (
                ReceiverState::Active { session_id },
                ControlFrame::Ping {
                    session_id: ping,
                    sequence,
                },
            ) if session_id == *ping => Ok(Some(ControlFrame::Pong {
                session_id,
                sequence: *sequence,
                highest_applied_sequence: 0,
            })),
            (
                ReceiverState::Ended { session_id },
                ControlFrame::EndSession {
                    session_id: end, ..
                },
            ) if session_id == *end => Ok(None),
            (
                ReceiverState::Ended { session_id },
                ControlFrame::Prepare {
                    request_id,
                    target_display: _,
                },
            ) => {
                self.state = ReceiverState::Prepared {
                    controller_boot: session_id.controller_boot,
                    request_id: *request_id,
                };
                Ok(Some(ControlFrame::Ready {
                    request_id: *request_id,
                    receiver_boot: self.local_boot,
                    input_ready: true,
                }))
            }
            (ReceiverState::AwaitHello, _) => Err(ProtocolError::InvalidTransition),
            (_, ControlFrame::Hello { boot_id, .. }) => match self.state {
                ReceiverState::Idle { controller_boot }
                | ReceiverState::Prepared {
                    controller_boot, ..
                } if controller_boot != *boot_id => Err(ProtocolError::WrongBoot),
                _ => Err(ProtocolError::InvalidTransition),
            },
            (_, ControlFrame::Commit { session_id, .. })
            | (_, ControlFrame::Ping { session_id, .. })
            | (_, ControlFrame::EndSession { session_id, .. }) => {
                if session_id.receiver_boot != self.local_boot {
                    Err(ProtocolError::WrongBoot)
                } else {
                    Err(ProtocolError::WrongSession)
                }
            }
            _ => Err(ProtocolError::InvalidTransition),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boot(value: u8) -> BootId {
        BootId([value; 16])
    }

    fn hello() -> ControlFrame {
        ControlFrame::Hello {
            boot_id: boot(1),
            peer_id: "controller-a".into(),
            role: DeviceRole::Controller,
            capabilities: vec!["control_v2".into(), "input_v2".into()],
        }
    }

    #[test]
    fn a13_rejects_wrong_major_without_downgrade() {
        let framed = encode_control(&hello()).unwrap();
        let mut payload = framed[4..].to_vec();
        let mut envelope: Envelope = rmp_serde::from_slice(&payload).unwrap();
        envelope.major = 1;
        payload = rmp_serde::to_vec_named(&envelope).unwrap();
        assert_eq!(
            decode_payload(&payload),
            Err(ProtocolError::UnsupportedMajor(1))
        );
    }

    #[test]
    fn a13_rejects_missing_capabilities_and_wrong_role() {
        let mut missing = hello();
        if let ControlFrame::Hello { capabilities, .. } = &mut missing {
            capabilities.pop();
        }
        assert_eq!(
            encode_control(&missing),
            Err(ProtocolError::InvalidField("capabilities"))
        );
        let mut receiver = ReceiverHandshake::new("controller-a".into(), boot(2)).unwrap();
        let wrong_role = ControlFrame::Hello {
            boot_id: boot(1),
            peer_id: "controller-a".into(),
            role: DeviceRole::Receiver,
            capabilities: vec!["control_v2".into(), "input_v2".into()],
        };
        assert_eq!(receiver.handle(&wrong_role), Err(ProtocolError::WrongRole));
    }

    #[test]
    fn a14_rejects_oversize_before_payload_allocation() {
        let mut decoder = ControlDecoder::default();
        let length = (MAX_CONTROL_FRAME_BYTES as u32 + 1).to_be_bytes();
        assert_eq!(
            decoder.push(&length),
            Err(ProtocolError::FrameTooLarge(MAX_CONTROL_FRAME_BYTES + 1))
        );
        assert!(decoder.payload.capacity() <= MAX_CONTROL_FRAME_BYTES);
    }

    #[test]
    fn a15_decodes_bytewise_and_multiple_frames() {
        let first = encode_control(&hello()).unwrap();
        let second = encode_control(&ControlFrame::Prepare {
            request_id: 7,
            target_display: "mac-main".into(),
        })
        .unwrap();
        let mut decoder = ControlDecoder::default();
        let mut output = Vec::new();
        for byte in first.iter().chain(second.iter()) {
            output.extend(decoder.push(&[*byte]).unwrap());
        }
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], hello());
        assert!(decoder.finish().is_ok());
    }

    #[test]
    fn a15_reports_truncated_stream() {
        let framed = encode_control(&hello()).unwrap();
        let mut decoder = ControlDecoder::default();
        assert!(decoder
            .push(&framed[..framed.len() - 1])
            .unwrap()
            .is_empty());
        assert_eq!(decoder.finish(), Err(ProtocolError::Truncated));
    }

    #[test]
    fn a16_old_boot_cannot_commit_new_receiver() {
        let mut receiver = ReceiverHandshake::new("controller-a".into(), boot(2)).unwrap();
        receiver.handle(&hello()).unwrap();
        receiver
            .handle(&ControlFrame::Prepare {
                request_id: 7,
                target_display: "mac-main".into(),
            })
            .unwrap();
        let stale = SessionId {
            controller_boot: boot(1),
            receiver_boot: boot(9),
            nonce: [3; 16],
        };
        assert_eq!(
            receiver.handle(&ControlFrame::Commit {
                request_id: 7,
                session_id: stale,
            }),
            Err(ProtocolError::WrongBoot)
        );
    }

    #[test]
    fn a17_ended_session_cannot_be_revived() {
        let mut receiver = ReceiverHandshake::new("controller-a".into(), boot(2)).unwrap();
        receiver.handle(&hello()).unwrap();
        receiver
            .handle(&ControlFrame::Prepare {
                request_id: 7,
                target_display: "mac-main".into(),
            })
            .unwrap();
        let session = SessionId {
            controller_boot: boot(1),
            receiver_boot: boot(2),
            nonce: [3; 16],
        };
        receiver
            .handle(&ControlFrame::Commit {
                request_id: 7,
                session_id: session,
            })
            .unwrap();
        receiver
            .handle(&ControlFrame::EndSession {
                session_id: session,
                reason: "local-return".into(),
            })
            .unwrap();
        assert_eq!(
            receiver.handle(&ControlFrame::Ping {
                session_id: session,
                sequence: 1,
            }),
            Err(ProtocolError::WrongSession)
        );
        assert!(receiver
            .handle(&ControlFrame::Commit {
                request_id: 7,
                session_id: session,
            })
            .is_err());
    }
}
