use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_MAJOR: u16 = 2;
pub const MAX_CONTROL_FRAME_BYTES: usize = 16 * 1024;
pub const MAX_CRITICAL_FRAME_BYTES: usize = 4 * 1024;
pub const MAX_MOTION_FRAME_BYTES: usize = 1024;
pub const MAX_BULK_FRAME_BYTES: usize = 2 * 1024 * 1024;
const MAGIC: u32 = u32::from_be_bytes(*b"MKV2");
const INPUT_MAGIC: u32 = u32::from_be_bytes(*b"MKI2");
const MOTION_MAGIC: u32 = u32::from_be_bytes(*b"MKM2");
const BULK_MAGIC: u32 = u32::from_be_bytes(*b"MKB2");
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

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BootId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClipboardOperationId {
    pub boot_id: BootId,
    pub local_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardTextOperation {
    pub operation_id: ClipboardOperationId,
    pub origin_peer: String,
    pub system_revision: u64,
    pub lamport: u64,
    pub digest: [u8; 32],
    pub text: String,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId {
    pub controller_boot: BootId,
    pub receiver_boot: BootId,
    pub nonce: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticalButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CriticalEvent {
    Key {
        key_code: u16,
        scan_code: u16,
        extended: bool,
        down: bool,
    },
    Button {
        button: CriticalButton,
        down: bool,
        x: i32,
        y: i32,
        motion_sequence: u64,
    },
    Scroll {
        delta_x: i32,
        delta_y: i32,
        x: i32,
        y: i32,
        motion_sequence: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriticalFrame {
    pub session_id: SessionId,
    pub sequence: u64,
    pub event: CriticalEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionFrame {
    pub session_id: SessionId,
    pub display_id: String,
    pub layout_revision: u64,
    pub sequence: u64,
    pub required_reliable_sequence: u64,
    pub x: i32,
    pub y: i32,
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
        layout_revision: u64,
    },
    Ready {
        request_id: u64,
        receiver_boot: BootId,
        input_ready: bool,
        target_display: String,
        layout_revision: u64,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CriticalEnvelope {
    magic: u32,
    major: u16,
    frame: CriticalFrame,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MotionEnvelope {
    magic: u32,
    major: u16,
    frame: MotionFrame,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BulkEnvelope {
    magic: u32,
    major: u16,
    operation: ClipboardTextOperation,
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

pub fn encode_critical(frame: &CriticalFrame) -> Result<Vec<u8>, ProtocolError> {
    validate_critical(frame)?;
    let payload = rmp_serde::to_vec_named(&CriticalEnvelope {
        magic: INPUT_MAGIC,
        major: PROTOCOL_MAJOR,
        frame: frame.clone(),
    })
    .map_err(|_| ProtocolError::Decode)?;
    if payload.is_empty() || payload.len() > MAX_CRITICAL_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(payload.len()));
    }
    let length = u32::try_from(payload.len()).map_err(|_| ProtocolError::InvalidLength)?;
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}

pub fn encode_motion(frame: &MotionFrame) -> Result<Vec<u8>, ProtocolError> {
    validate_motion(frame)?;
    let payload = rmp_serde::to_vec_named(&MotionEnvelope {
        magic: MOTION_MAGIC,
        major: PROTOCOL_MAJOR,
        frame: frame.clone(),
    })
    .map_err(|_| ProtocolError::Decode)?;
    if payload.is_empty() || payload.len() > MAX_MOTION_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(payload.len()));
    }
    Ok(payload)
}

pub fn clipboard_text_digest(bytes: &[u8]) -> [u8; 32] {
    let value = digest::digest(&digest::SHA256, bytes);
    let mut result = [0_u8; 32];
    result.copy_from_slice(value.as_ref());
    result
}

impl ClipboardTextOperation {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        validate_clipboard_text_operation(self)?;
        let payload = rmp_serde::to_vec_named(&BulkEnvelope {
            magic: BULK_MAGIC,
            major: PROTOCOL_MAJOR,
            operation: self.clone(),
        })
        .map_err(|_| ProtocolError::Decode)?;
        if payload.is_empty() || payload.len() > MAX_BULK_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(payload.len()));
        }
        Ok(payload)
    }

    pub fn decode(payload: &[u8]) -> Result<Self, ProtocolError> {
        if payload.is_empty() {
            return Err(ProtocolError::InvalidLength);
        }
        if payload.len() > MAX_BULK_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(payload.len()));
        }
        let envelope: BulkEnvelope =
            rmp_serde::from_slice(payload).map_err(|_| ProtocolError::Decode)?;
        if envelope.magic != BULK_MAGIC {
            return Err(ProtocolError::InvalidMagic);
        }
        if envelope.major != PROTOCOL_MAJOR {
            return Err(ProtocolError::UnsupportedMajor(envelope.major));
        }
        validate_clipboard_text_operation(&envelope.operation)?;
        Ok(envelope.operation)
    }
}

fn validate_clipboard_text_operation(
    operation: &ClipboardTextOperation,
) -> Result<(), ProtocolError> {
    validate_text(&operation.origin_peer, MAX_PEER_ID_BYTES, "origin_peer")?;
    if operation.operation_id.local_sequence == 0 {
        return Err(ProtocolError::InvalidField("local_sequence"));
    }
    if operation.lamport == 0 {
        return Err(ProtocolError::InvalidField("lamport"));
    }
    if operation.text.is_empty() {
        return Err(ProtocolError::InvalidField("text"));
    }
    if operation.digest != clipboard_text_digest(operation.text.as_bytes()) {
        return Err(ProtocolError::InvalidField("digest"));
    }
    Ok(())
}

pub fn decode_motion(payload: &[u8]) -> Result<MotionFrame, ProtocolError> {
    if payload.is_empty() {
        return Err(ProtocolError::InvalidLength);
    }
    if payload.len() > MAX_MOTION_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(payload.len()));
    }
    let envelope: MotionEnvelope =
        rmp_serde::from_slice(payload).map_err(|_| ProtocolError::Decode)?;
    if envelope.magic != MOTION_MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    if envelope.major != PROTOCOL_MAJOR {
        return Err(ProtocolError::UnsupportedMajor(envelope.major));
    }
    validate_motion(&envelope.frame)?;
    Ok(envelope.frame)
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

#[derive(Default)]
pub struct CriticalDecoder {
    prefix: [u8; 4],
    prefix_len: usize,
    expected: Option<usize>,
    payload: Vec<u8>,
}

impl CriticalDecoder {
    pub fn push(&mut self, mut bytes: &[u8]) -> Result<Vec<CriticalFrame>, ProtocolError> {
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
                if length > MAX_CRITICAL_FRAME_BYTES {
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
                decoded.push(decode_critical_payload(&self.payload)?);
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

fn decode_critical_payload(payload: &[u8]) -> Result<CriticalFrame, ProtocolError> {
    let envelope: CriticalEnvelope =
        rmp_serde::from_slice(payload).map_err(|_| ProtocolError::Decode)?;
    if envelope.magic != INPUT_MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    if envelope.major != PROTOCOL_MAJOR {
        return Err(ProtocolError::UnsupportedMajor(envelope.major));
    }
    validate_critical(&envelope.frame)?;
    Ok(envelope.frame)
}

fn validate_critical(frame: &CriticalFrame) -> Result<(), ProtocolError> {
    validate_session(&frame.session_id)?;
    if frame.sequence == 0 {
        return Err(ProtocolError::InvalidField("sequence"));
    }
    match &frame.event {
        CriticalEvent::Key {
            key_code,
            scan_code,
            ..
        } if *key_code == 0 && *scan_code == 0 => Err(ProtocolError::InvalidField("key_code")),
        CriticalEvent::Button {
            motion_sequence, ..
        }
        | CriticalEvent::Scroll {
            motion_sequence, ..
        } if *motion_sequence == 0 => Err(ProtocolError::InvalidField("motion_sequence")),
        CriticalEvent::Scroll {
            delta_x, delta_y, ..
        } if *delta_x == 0 && *delta_y == 0 => Err(ProtocolError::InvalidField("scroll_delta")),
        _ => Ok(()),
    }
}

fn validate_motion(frame: &MotionFrame) -> Result<(), ProtocolError> {
    validate_session(&frame.session_id)?;
    validate_text(&frame.display_id, MAX_PEER_ID_BYTES, "display_id")?;
    if frame.layout_revision == 0 {
        return Err(ProtocolError::InvalidField("layout_revision"));
    }
    if frame.sequence == 0 {
        return Err(ProtocolError::InvalidField("motion_sequence"));
    }
    Ok(())
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
            layout_revision,
        } => {
            if *request_id == 0 {
                return Err(ProtocolError::InvalidField("request_id"));
            }
            validate_text(target_display, MAX_PEER_ID_BYTES, "target_display")?;
            if *layout_revision == 0 {
                return Err(ProtocolError::InvalidField("layout_revision"));
            }
        }
        ControlFrame::Ready {
            request_id,
            target_display,
            layout_revision,
            ..
        } => {
            if *request_id == 0 {
                return Err(ProtocolError::InvalidField("request_id"));
            }
            validate_text(target_display, MAX_PEER_ID_BYTES, "target_display")?;
            if *layout_revision == 0 {
                return Err(ProtocolError::InvalidField("layout_revision"));
            }
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum ControllerState {
    Idle,
    AwaitReady {
        request_id: u64,
        target_display: String,
        layout_revision: u64,
    },
    Ready {
        request_id: u64,
        session_id: SessionId,
    },
    AwaitCommitAck {
        session_id: SessionId,
    },
    Active {
        session_id: SessionId,
    },
    Ended,
}

pub struct ControllerHandshake {
    local_boot: BootId,
    local_peer_id: String,
    state: ControllerState,
    ping_sequence: u64,
    last_pong_sequence: u64,
    highest_remote_applied: u64,
}

impl ControllerHandshake {
    pub fn new(local_boot: BootId, local_peer_id: String) -> Result<Self, ProtocolError> {
        if local_boot.0 == [0; 16] {
            return Err(ProtocolError::InvalidField("boot_id"));
        }
        validate_text(&local_peer_id, MAX_PEER_ID_BYTES, "peer_id")?;
        Ok(Self {
            local_boot,
            local_peer_id,
            state: ControllerState::Idle,
            ping_sequence: 0,
            last_pong_sequence: 0,
            highest_remote_applied: 0,
        })
    }

    pub fn begin(
        &mut self,
        request_id: u64,
        target_display: String,
        layout_revision: u64,
    ) -> Result<Vec<ControlFrame>, ProtocolError> {
        if !matches!(self.state, ControllerState::Idle | ControllerState::Ended) {
            return Err(ProtocolError::InvalidTransition);
        }
        let prepare = ControlFrame::Prepare {
            request_id,
            target_display: target_display.clone(),
            layout_revision,
        };
        validate_frame(&prepare)?;
        self.state = ControllerState::AwaitReady {
            request_id,
            target_display,
            layout_revision,
        };
        Ok(vec![
            ControlFrame::Hello {
                boot_id: self.local_boot,
                peer_id: self.local_peer_id.clone(),
                role: DeviceRole::Controller,
                capabilities: REQUIRED_CAPABILITIES
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
            },
            prepare,
        ])
    }

    pub fn handle(&mut self, frame: &ControlFrame) -> Result<Option<ControlFrame>, ProtocolError> {
        validate_frame(frame)?;
        match (self.state.clone(), frame) {
            (
                ControllerState::AwaitReady {
                    request_id,
                    target_display,
                    layout_revision,
                },
                ControlFrame::Ready {
                    request_id: received,
                    receiver_boot,
                    input_ready: true,
                    target_display: received_display,
                    layout_revision: received_revision,
                },
            ) if request_id == *received
                && target_display == *received_display
                && layout_revision == *received_revision
                && receiver_boot.0 != [0; 16] =>
            {
                let session_id = SessionId::generate(self.local_boot, *receiver_boot)?;
                self.state = ControllerState::Ready {
                    request_id,
                    session_id,
                };
                Ok(None)
            }
            (
                ControllerState::AwaitCommitAck { session_id },
                ControlFrame::CommitAck {
                    session_id: received,
                },
            ) if session_id == *received => {
                self.state = ControllerState::Active { session_id };
                self.ping_sequence = 0;
                self.last_pong_sequence = 0;
                self.highest_remote_applied = 0;
                Ok(None)
            }
            (
                ControllerState::Active { session_id },
                ControlFrame::Pong {
                    session_id: received,
                    sequence,
                    highest_applied_sequence,
                },
            ) if session_id == *received
                && *sequence > self.last_pong_sequence
                && *sequence <= self.ping_sequence
                && *highest_applied_sequence >= self.highest_remote_applied =>
            {
                self.last_pong_sequence = *sequence;
                self.highest_remote_applied = *highest_applied_sequence;
                Ok(None)
            }
            (_, ControlFrame::Reject { .. }) => {
                self.state = ControllerState::Ended;
                Err(ProtocolError::InvalidTransition)
            }
            (ControllerState::AwaitReady { .. }, ControlFrame::Ready { .. }) => {
                Err(ProtocolError::WrongSession)
            }
            (ControllerState::AwaitCommitAck { .. }, ControlFrame::CommitAck { .. }) => {
                Err(ProtocolError::WrongSession)
            }
            (ControllerState::Active { .. }, ControlFrame::Pong { .. }) => {
                Err(ProtocolError::WrongSession)
            }
            _ => Err(ProtocolError::InvalidTransition),
        }
    }

    pub fn active_session(&self) -> Option<SessionId> {
        match self.state {
            ControllerState::Active { session_id } => Some(session_id),
            _ => None,
        }
    }

    pub fn prepared_session(&self) -> Option<(u64, SessionId)> {
        match self.state {
            ControllerState::Ready {
                request_id,
                session_id,
            } => Some((request_id, session_id)),
            _ => None,
        }
    }

    pub fn commit(
        &mut self,
        request_id: u64,
        session_id: SessionId,
    ) -> Result<ControlFrame, ProtocolError> {
        if self.prepared_session() != Some((request_id, session_id)) {
            return Err(ProtocolError::WrongSession);
        }
        self.state = ControllerState::AwaitCommitAck { session_id };
        Ok(ControlFrame::Commit {
            request_id,
            session_id,
        })
    }

    pub fn ping(&mut self) -> Result<ControlFrame, ProtocolError> {
        let session_id = self
            .active_session()
            .ok_or(ProtocolError::InvalidTransition)?;
        self.ping_sequence = self
            .ping_sequence
            .checked_add(1)
            .ok_or(ProtocolError::InvalidField("sequence"))?;
        Ok(ControlFrame::Ping {
            session_id,
            sequence: self.ping_sequence,
        })
    }

    pub fn highest_remote_applied(&self) -> u64 {
        self.highest_remote_applied
    }

    pub fn abort(&mut self) {
        self.state = ControllerState::Ended;
    }

    pub fn end(&mut self, reason: String) -> Result<ControlFrame, ProtocolError> {
        let session_id = self
            .active_session()
            .ok_or(ProtocolError::InvalidTransition)?;
        let frame = ControlFrame::EndSession { session_id, reason };
        validate_frame(&frame)?;
        self.state = ControllerState::Ended;
        Ok(frame)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputSessionGate {
    local_boot: BootId,
    active: Option<(SessionId, u64)>,
    ended: Option<SessionId>,
}

impl InputSessionGate {
    pub fn new(local_boot: BootId) -> Self {
        Self {
            local_boot,
            active: None,
            ended: None,
        }
    }

    pub fn activate(&mut self, session_id: SessionId) -> Result<(), ProtocolError> {
        validate_session(&session_id)?;
        if session_id.receiver_boot != self.local_boot {
            return Err(ProtocolError::WrongBoot);
        }
        if self.ended == Some(session_id) {
            return Err(ProtocolError::WrongSession);
        }
        self.active = Some((session_id, 1));
        Ok(())
    }

    pub fn accept(&mut self, frame: &CriticalFrame) -> Result<(), ProtocolError> {
        validate_critical(frame)?;
        let Some((session_id, next_sequence)) = self.active else {
            return Err(ProtocolError::WrongSession);
        };
        if frame.session_id.receiver_boot != self.local_boot {
            return Err(ProtocolError::WrongBoot);
        }
        if frame.session_id != session_id || frame.sequence != next_sequence {
            return Err(ProtocolError::WrongSession);
        }
        let following_sequence = next_sequence
            .checked_add(1)
            .ok_or(ProtocolError::InvalidField("sequence"))?;
        self.active = Some((session_id, following_sequence));
        Ok(())
    }

    pub fn end(&mut self, session_id: SessionId) -> Result<(), ProtocolError> {
        let Some((active, _)) = self.active else {
            return Err(ProtocolError::WrongSession);
        };
        if active != session_id {
            return Err(ProtocolError::WrongSession);
        }
        self.active = None;
        self.ended = Some(session_id);
        Ok(())
    }
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

    pub fn abort_active(&mut self, session_id: SessionId) -> Result<(), ProtocolError> {
        match self.state {
            ReceiverState::Active { session_id: active } if active == session_id => {
                self.state = ReceiverState::Ended { session_id };
                Ok(())
            }
            _ => Err(ProtocolError::WrongSession),
        }
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
                    target_display,
                    layout_revision,
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
                    target_display: target_display.clone(),
                    layout_revision: *layout_revision,
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
                    target_display,
                    layout_revision,
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
                    target_display: target_display.clone(),
                    layout_revision: *layout_revision,
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
            layout_revision: 1,
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
                layout_revision: 1,
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
                layout_revision: 1,
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

    #[test]
    fn critical_frames_are_bounded_incremental_and_session_scoped() {
        let session = SessionId {
            controller_boot: boot(1),
            receiver_boot: boot(2),
            nonce: [3; 16],
        };
        let first = CriticalFrame {
            session_id: session,
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        let second = CriticalFrame {
            session_id: session,
            sequence: 2,
            event: CriticalEvent::Button {
                button: CriticalButton::Left,
                down: true,
                x: 320,
                y: 240,
                motion_sequence: 9,
            },
        };
        let bytes = [
            encode_critical(&first).unwrap(),
            encode_critical(&second).unwrap(),
        ]
        .concat();
        let mut decoder = CriticalDecoder::default();
        let mut decoded = Vec::new();
        for chunk in bytes.chunks(3) {
            decoded.extend(decoder.push(chunk).unwrap());
        }
        assert_eq!(decoded, vec![first, second]);
        assert!(decoder.finish().is_ok());
    }

    #[test]
    fn critical_frames_reject_zero_sequence_and_oversize_prefix() {
        let invalid = CriticalFrame {
            session_id: SessionId {
                controller_boot: boot(1),
                receiver_boot: boot(2),
                nonce: [3; 16],
            },
            sequence: 0,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        assert_eq!(
            encode_critical(&invalid),
            Err(ProtocolError::InvalidField("sequence"))
        );
        let mut decoder = CriticalDecoder::default();
        assert_eq!(
            decoder.push(&((MAX_CRITICAL_FRAME_BYTES as u32) + 1).to_be_bytes()),
            Err(ProtocolError::FrameTooLarge(MAX_CRITICAL_FRAME_BYTES + 1))
        );

        let invalid_key = CriticalFrame {
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 0,
                scan_code: 0,
                extended: false,
                down: true,
            },
            ..invalid
        };
        assert_eq!(
            encode_critical(&invalid_key),
            Err(ProtocolError::InvalidField("key_code"))
        );
    }

    #[test]
    fn input_gate_fails_closed_when_sequence_space_is_exhausted() {
        let session = SessionId {
            controller_boot: boot(1),
            receiver_boot: boot(2),
            nonce: [3; 16],
        };
        let mut gate = InputSessionGate::new(boot(2));
        gate.active = Some((session, u64::MAX));
        let last = CriticalFrame {
            session_id: session,
            sequence: u64::MAX,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: false,
            },
        };
        assert_eq!(
            gate.accept(&last),
            Err(ProtocolError::InvalidField("sequence"))
        );
        assert_eq!(gate.active, Some((session, u64::MAX)));
    }

    #[test]
    fn a16_input_gate_rejects_previous_receiver_boot_without_applying() {
        let stale_session = SessionId {
            controller_boot: boot(1),
            receiver_boot: boot(2),
            nonce: [3; 16],
        };
        let mut gate = InputSessionGate::new(boot(9));
        assert_eq!(gate.activate(stale_session), Err(ProtocolError::WrongBoot));
        let stale = CriticalFrame {
            session_id: stale_session,
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        let mut applied = 0;
        if gate.accept(&stale).is_ok() {
            applied += 1;
        }
        assert_eq!(applied, 0);
    }

    #[test]
    fn a17_input_gate_rejects_frames_after_end() {
        let session = SessionId {
            controller_boot: boot(1),
            receiver_boot: boot(2),
            nonce: [3; 16],
        };
        let mut gate = InputSessionGate::new(boot(2));
        gate.activate(session).unwrap();
        gate.end(session).unwrap();
        let late = CriticalFrame {
            session_id: session,
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        assert_eq!(gate.accept(&late), Err(ProtocolError::WrongSession));
        assert_eq!(gate.activate(session), Err(ProtocolError::WrongSession));
    }

    fn motion(sequence: u64, required_reliable_sequence: u64) -> MotionFrame {
        MotionFrame {
            session_id: SessionId {
                controller_boot: boot(1),
                receiver_boot: boot(2),
                nonce: [9; 16],
            },
            display_id: "mac-main".into(),
            layout_revision: 1,
            sequence,
            required_reliable_sequence,
            x: 1920,
            y: 1080,
        }
    }

    #[test]
    fn motion_datagram_round_trips_without_stream_prefix() {
        let frame = motion(7, 4);
        let encoded = encode_motion(&frame).unwrap();
        assert!(encoded.len() <= MAX_MOTION_FRAME_BYTES);
        assert_eq!(decode_motion(&encoded), Ok(frame));
    }

    #[test]
    fn motion_datagram_rejects_zero_sequence_and_size_before_decode() {
        assert_eq!(
            encode_motion(&motion(0, 0)),
            Err(ProtocolError::InvalidField("motion_sequence"))
        );
        assert_eq!(
            decode_motion(&vec![0; MAX_MOTION_FRAME_BYTES + 1]),
            Err(ProtocolError::FrameTooLarge(MAX_MOTION_FRAME_BYTES + 1))
        );
        let mut stale_layout = motion(1, 0);
        stale_layout.layout_revision = 0;
        assert_eq!(
            encode_motion(&stale_layout),
            Err(ProtocolError::InvalidField("layout_revision"))
        );
    }

    #[test]
    fn motion_datagram_rejects_wrong_magic_and_major() {
        let wrong_magic = rmp_serde::to_vec_named(&MotionEnvelope {
            magic: 0,
            major: PROTOCOL_MAJOR,
            frame: motion(1, 0),
        })
        .unwrap();
        assert_eq!(
            decode_motion(&wrong_magic),
            Err(ProtocolError::InvalidMagic)
        );
        let wrong_major = rmp_serde::to_vec_named(&MotionEnvelope {
            magic: MOTION_MAGIC,
            major: PROTOCOL_MAJOR + 1,
            frame: motion(1, 0),
        })
        .unwrap();
        assert_eq!(
            decode_motion(&wrong_major),
            Err(ProtocolError::UnsupportedMajor(PROTOCOL_MAJOR + 1))
        );
    }

    #[test]
    fn controller_handshake_generates_fresh_session_and_requires_exact_ack() {
        let mut controller = ControllerHandshake::new(boot(1), "controller-a".into()).unwrap();
        let begin = controller.begin(7, "mac-main".into(), 1).unwrap();
        assert!(matches!(begin[0], ControlFrame::Hello { .. }));
        assert_eq!(
            begin[1],
            ControlFrame::Prepare {
                request_id: 7,
                target_display: "mac-main".into(),
                layout_revision: 1,
            }
        );
        assert_eq!(
            controller
                .handle(&ControlFrame::Ready {
                    request_id: 7,
                    receiver_boot: boot(2),
                    input_ready: true,
                    target_display: "mac-main".into(),
                    layout_revision: 1,
                })
                .unwrap(),
            None
        );
        let (request_id, prepared_session) = controller.prepared_session().unwrap();
        let commit = controller.commit(request_id, prepared_session).unwrap();
        let ControlFrame::Commit { session_id, .. } = commit else {
            panic!("commit")
        };
        assert_eq!(session_id.controller_boot, boot(1));
        assert_eq!(session_id.receiver_boot, boot(2));
        assert_ne!(session_id.nonce, [0; 16]);
        let wrong = SessionId {
            nonce: [9; 16],
            ..session_id
        };
        assert_eq!(
            controller.handle(&ControlFrame::CommitAck { session_id: wrong }),
            Err(ProtocolError::WrongSession)
        );
        controller
            .handle(&ControlFrame::CommitAck { session_id })
            .unwrap();
        assert_eq!(controller.active_session(), Some(session_id));
    }

    #[test]
    fn controller_rejects_unready_or_stale_ready_and_scopes_ping_end() {
        let mut controller = ControllerHandshake::new(boot(1), "controller-a".into()).unwrap();
        controller.begin(7, "mac-main".into(), 1).unwrap();
        assert_eq!(
            controller.handle(&ControlFrame::Ready {
                request_id: 7,
                receiver_boot: boot(2),
                input_ready: true,
                target_display: "mac-main".into(),
                layout_revision: 2,
            }),
            Err(ProtocolError::WrongSession)
        );
        assert_eq!(
            controller.handle(&ControlFrame::Ready {
                request_id: 8,
                receiver_boot: boot(2),
                input_ready: true,
                target_display: "mac-main".into(),
                layout_revision: 1,
            }),
            Err(ProtocolError::WrongSession)
        );
        assert_eq!(
            controller.handle(&ControlFrame::Ready {
                request_id: 7,
                receiver_boot: boot(2),
                input_ready: false,
                target_display: "mac-main".into(),
                layout_revision: 1,
            }),
            Err(ProtocolError::WrongSession)
        );
        assert_eq!(
            controller
                .handle(&ControlFrame::Ready {
                    request_id: 7,
                    receiver_boot: boot(2),
                    input_ready: true,
                    target_display: "mac-main".into(),
                    layout_revision: 1,
                })
                .unwrap(),
            None
        );
        let (request_id, prepared_session) = controller.prepared_session().unwrap();
        let commit = controller.commit(request_id, prepared_session).unwrap();
        let ControlFrame::Commit { session_id, .. } = commit else {
            panic!("commit")
        };
        assert!(controller.ping().is_err());
        controller
            .handle(&ControlFrame::CommitAck { session_id })
            .unwrap();
        assert!(matches!(
            controller.ping().unwrap(),
            ControlFrame::Ping { sequence: 1, .. }
        ));
        controller
            .handle(&ControlFrame::Pong {
                session_id,
                sequence: 1,
                highest_applied_sequence: 12,
            })
            .unwrap();
        assert_eq!(controller.highest_remote_applied(), 12);
        assert!(controller
            .handle(&ControlFrame::Pong {
                session_id,
                sequence: 1,
                highest_applied_sequence: 13,
            })
            .is_err());
        assert!(matches!(
            controller.end("return-windows".into()).unwrap(),
            ControlFrame::EndSession { .. }
        ));
        assert!(controller.ping().is_err());
        assert!(controller.end("again".into()).is_err());
    }
}
