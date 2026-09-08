use crate::{
    control_ports::{submit_ready, InjectorPort, PortError},
    pressed_state::PressedState,
    protocol_v2::{
        BootId, ControlFrame, CriticalFrame, DeviceRole, InputSessionGate, MotionFrame,
        ProtocolError, ReceiverHandshake, SessionId,
    },
    quic_transport::{AuthenticatedPeer, PeerRole},
    shared_input::InputCommand,
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
    LayoutChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionHealth {
    pub active: bool,
    pub highest_applied_sequence: u64,
    pub last_fault: Option<SessionFault>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionDisposition {
    Applied,
    Buffered,
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiverDisplayLayout {
    pub display_id: String,
    pub layout_revision: u64,
    pub logical_width: i32,
    pub logical_height: i32,
    pub native_x: i32,
    pub native_y: i32,
    pub native_width: i32,
    pub native_height: i32,
}

impl ReceiverDisplayLayout {
    fn map(&self, x: i32, y: i32) -> Result<(i32, i32), ProtocolError> {
        if self.layout_revision == 0
            || self.logical_width <= 0
            || self.logical_height <= 0
            || self.native_width <= 0
            || self.native_height <= 0
            || x < 0
            || y < 0
            || x >= self.logical_width
            || y >= self.logical_height
        {
            return Err(ProtocolError::InvalidField("coordinates"));
        }
        Ok((
            map_axis(x, self.logical_width, self.native_x, self.native_width),
            map_axis(y, self.logical_height, self.native_y, self.native_height),
        ))
    }
}

fn map_axis(value: i32, logical_size: i32, native_origin: i32, native_size: i32) -> i32 {
    if logical_size <= 1 || native_size <= 1 {
        return native_origin;
    }
    let offset = i64::from(value) * i64::from(native_size - 1) / i64::from(logical_size - 1);
    i32::try_from(i64::from(native_origin) + offset).unwrap_or(i32::MAX)
}

pub fn display_layout_revision(display_id: &str, width: i32, height: i32, scale_bits: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in display_id
        .as_bytes()
        .iter()
        .copied()
        .chain(width.to_le_bytes())
        .chain(height.to_le_bytes())
        .chain(scale_bits.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash.max(1)
}

pub fn remap_modifier_vk(vk: u16, control: &str, alt: &str, meta: &str) -> u16 {
    let target = match vk {
        0x11 | 0xA2 | 0xA3 => control,
        0x12 | 0xA4 | 0xA5 => alt,
        0x5B | 0x5C => meta,
        _ => return vk,
    };
    let right = matches!(vk, 0xA1 | 0xA3 | 0xA5 | 0x5C);
    let generic = matches!(vk, 0x10 | 0x11 | 0x12);
    match target {
        "control" => {
            if generic {
                0x11
            } else if right {
                0xA3
            } else {
                0xA2
            }
        }
        "alt" => {
            if generic {
                0x12
            } else if right {
                0xA5
            } else {
                0xA4
            }
        }
        "meta" => {
            if right {
                0x5C
            } else {
                0x5B
            }
        }
        _ => vk,
    }
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
    highest_motion_sequence: u64,
    pending_motion: Option<MotionFrame>,
    display_layouts: Vec<ReceiverDisplayLayout>,
    prepared_display: Option<ReceiverDisplayLayout>,
    active_display: Option<ReceiverDisplayLayout>,
    modifier_remap: bool,
    modifier_control: String,
    modifier_alt: String,
    modifier_meta: String,
    last_activity_ms: Option<u64>,
    last_fault: Option<SessionFault>,
    pressed: PressedState,
    injector: I,
}

impl<I: InjectorPort> ReceiverSessionRuntime<I> {
    #[cfg(test)]
    pub fn new(local_boot: BootId, injector: I) -> Self {
        Self::new_with_display_layouts(
            local_boot,
            injector,
            vec![ReceiverDisplayLayout {
                display_id: "mac-main".into(),
                layout_revision: 1,
                logical_width: 1920,
                logical_height: 1080,
                native_x: 0,
                native_y: 0,
                native_width: 1920,
                native_height: 1080,
            }],
        )
    }

    pub fn new_with_display_layouts(
        local_boot: BootId,
        injector: I,
        display_layouts: Vec<ReceiverDisplayLayout>,
    ) -> Self {
        Self {
            local_boot,
            binding: None,
            handshake: None,
            input_gate: InputSessionGate::new(local_boot),
            active_session: None,
            highest_applied_sequence: 0,
            highest_motion_sequence: 0,
            pending_motion: None,
            display_layouts,
            prepared_display: None,
            active_display: None,
            modifier_remap: false,
            modifier_control: "same".into(),
            modifier_alt: "same".into(),
            modifier_meta: "same".into(),
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
            self.reset_motion();
            self.prepared_display = None;
            self.active_display = None;
            self.last_activity_ms = None;
            return Ok(response);
        }

        if self.binding.as_ref() != Some(peer) {
            return Err(SessionRuntimeError::WrongConnection);
        }
        if self.active_session.is_none()
            && matches!(frame, ControlFrame::Commit { .. })
            && self.prepared_display.is_none()
        {
            return Err(SessionRuntimeError::Protocol(ProtocolError::InvalidField(
                "layout_revision",
            )));
        }
        if let ControlFrame::Prepare {
            target_display,
            layout_revision,
            ..
        } = frame
        {
            self.injector
                .readiness()
                .map_err(SessionRuntimeError::Injector)?;
            self.prepared_display = self
                .display_layouts
                .iter()
                .find(|layout| {
                    layout.display_id == *target_display
                        && layout.layout_revision == *layout_revision
                })
                .cloned();
            if self.prepared_display.is_none() {
                return Err(SessionRuntimeError::Protocol(ProtocolError::InvalidField(
                    "layout_revision",
                )));
            }
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
                self.active_display = self.prepared_display.take();
                self.highest_applied_sequence = 0;
                self.reset_motion();
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
                self.active_display = None;
                self.last_activity_ms = None;
                self.reset_motion();
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

    pub fn update_display_layouts(
        &mut self,
        display_layouts: Vec<ReceiverDisplayLayout>,
    ) -> Result<bool, SessionRuntimeError> {
        let active_is_current = self.active_display.as_ref().map_or(true, |active| {
            display_layouts.iter().any(|layout| layout == active)
        });
        let prepared_is_current = self.prepared_display.as_ref().map_or(true, |prepared| {
            display_layouts.iter().any(|layout| layout == prepared)
        });
        self.display_layouts = display_layouts;
        if !prepared_is_current {
            self.prepared_display = None;
        }
        if active_is_current {
            return Ok(false);
        }
        if let Some(session_id) = self.active_session.take() {
            let _ = self.input_gate.end(session_id);
            if let Some(handshake) = self.handshake.as_mut() {
                let _ = handshake.abort_active(session_id);
            }
        }
        self.active_display = None;
        self.last_activity_ms = None;
        self.reset_motion();
        self.last_fault = Some(SessionFault::LayoutChanged);
        self.release_pressed()?;
        Ok(true)
    }

    pub fn update_modifier_mapping(&mut self, enabled: bool, control: &str, alt: &str, meta: &str) {
        self.modifier_remap = enabled;
        self.modifier_control = control.into();
        self.modifier_alt = alt.into();
        self.modifier_meta = meta.into();
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
        let motion_snapshot = critical_motion_snapshot(&frame.event);
        if motion_snapshot.is_some_and(|(sequence, _, _)| {
            sequence != 0 && sequence < self.highest_motion_sequence
        }) {
            return Err(SessionRuntimeError::Protocol(ProtocolError::WrongSession));
        }
        let mut mapped_event = frame.event.clone();
        if let crate::protocol_v2::CriticalEvent::Key { key_code, .. } = &mut mapped_event {
            if self.modifier_remap {
                *key_code = remap_modifier_vk(
                    *key_code,
                    &self.modifier_control,
                    &self.modifier_alt,
                    &self.modifier_meta,
                );
            }
        }
        if let Some((_, x, y)) = motion_snapshot {
            let (x, y) = self.map_active_pointer(x, y)?;
            match &mut mapped_event {
                crate::protocol_v2::CriticalEvent::Button {
                    x: event_x,
                    y: event_y,
                    ..
                }
                | crate::protocol_v2::CriticalEvent::Scroll {
                    x: event_x,
                    y: event_y,
                    ..
                } => {
                    *event_x = x;
                    *event_y = y;
                }
                crate::protocol_v2::CriticalEvent::Key { .. } => {}
            }
        }
        self.input_gate
            .accept(frame)
            .map_err(SessionRuntimeError::Protocol)?;
        let pressed_before = self.pressed.clone();
        let mut commands = Vec::new();
        if let Some((_, x, y)) = motion_snapshot {
            let (x, y) = self.map_active_pointer(x, y)?;
            let drag_button = self.pressed.drag_button();
            self.pressed.update_pointer(x, y);
            if matches!(
                frame.event,
                crate::protocol_v2::CriticalEvent::Scroll { .. }
            ) {
                commands.push(InputCommand::MouseMove { x, y, drag_button });
            }
        }
        commands.extend(self.pressed.apply(&mapped_event));
        if commands.is_empty() {
            if let Some((_, x, y)) = motion_snapshot {
                let (x, y) = self.map_active_pointer(x, y)?;
                commands.push(InputCommand::MouseMove {
                    x,
                    y,
                    drag_button: self.pressed.drag_button(),
                });
            }
        }
        for command in commands {
            if let Err(error) = submit_ready(&mut self.injector, command) {
                self.pressed = pressed_before;
                self.last_fault = Some(SessionFault::Injector(error));
                self.abort_after_injector_failure(frame.session_id);
                return Err(SessionRuntimeError::Injector(error));
            }
        }
        self.highest_applied_sequence = frame.sequence;
        if let Some((sequence, _, _)) = motion_snapshot {
            self.highest_motion_sequence = self.highest_motion_sequence.max(sequence);
        }
        self.last_activity_ms = Some(now_ms);
        self.flush_pending_motion()?;
        Ok(())
    }

    pub fn handle_motion_at(
        &mut self,
        frame: MotionFrame,
        peer: &AuthenticatedPeer,
        now_ms: u64,
    ) -> Result<MotionDisposition, SessionRuntimeError> {
        if self.binding.as_ref() != Some(peer) || peer.role != PeerRole::Controller {
            return Err(SessionRuntimeError::WrongConnection);
        }
        if frame.sequence == 0
            || self.active_session != Some(frame.session_id)
            || frame.session_id.receiver_boot != self.local_boot
        {
            return Err(SessionRuntimeError::Protocol(ProtocolError::WrongSession));
        }
        if frame.sequence <= self.highest_motion_sequence {
            return Ok(MotionDisposition::Stale);
        }
        let active_display = self
            .active_display
            .as_ref()
            .ok_or(SessionRuntimeError::Protocol(ProtocolError::InvalidField(
                "display_id",
            )))?;
        if frame.display_id != active_display.display_id
            || frame.layout_revision != active_display.layout_revision
        {
            return Err(SessionRuntimeError::Protocol(ProtocolError::InvalidField(
                "layout_revision",
            )));
        }
        active_display
            .map(frame.x, frame.y)
            .map_err(SessionRuntimeError::Protocol)?;
        self.last_activity_ms = Some(now_ms);
        if frame.required_reliable_sequence > self.highest_applied_sequence {
            if self
                .pending_motion
                .as_ref()
                .map_or(true, |pending| frame.sequence > pending.sequence)
            {
                self.pending_motion = Some(frame);
            }
            return Ok(MotionDisposition::Buffered);
        }
        self.apply_motion(frame)?;
        Ok(MotionDisposition::Applied)
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
        self.reset_motion();
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
        self.reset_motion();
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
        self.reset_motion();
        let _ = self.release_pressed();
    }

    fn apply_motion(&mut self, frame: MotionFrame) -> Result<(), SessionRuntimeError> {
        let (x, y) = self.map_active_pointer(frame.x, frame.y)?;
        let pressed_before = self.pressed.clone();
        self.pressed.update_pointer(x, y);
        let command = InputCommand::MouseMove {
            x,
            y,
            drag_button: self.pressed.drag_button(),
        };
        if let Err(error) = submit_ready(&mut self.injector, command) {
            self.pressed = pressed_before;
            self.last_fault = Some(SessionFault::Injector(error));
            self.abort_after_injector_failure(frame.session_id);
            return Err(SessionRuntimeError::Injector(error));
        }
        self.highest_motion_sequence = frame.sequence;
        Ok(())
    }

    fn flush_pending_motion(&mut self) -> Result<(), SessionRuntimeError> {
        let Some(frame) = self.pending_motion.clone() else {
            return Ok(());
        };
        if frame.required_reliable_sequence > self.highest_applied_sequence {
            return Ok(());
        }
        self.pending_motion = None;
        if frame.sequence > self.highest_motion_sequence {
            self.apply_motion(frame)?;
        }
        Ok(())
    }

    fn reset_motion(&mut self) {
        self.highest_motion_sequence = 0;
        self.pending_motion = None;
    }

    fn map_active_pointer(&self, x: i32, y: i32) -> Result<(i32, i32), SessionRuntimeError> {
        self.active_display
            .as_ref()
            .ok_or(SessionRuntimeError::Protocol(ProtocolError::InvalidField(
                "display_id",
            )))?
            .map(x, y)
            .map_err(SessionRuntimeError::Protocol)
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

fn critical_motion_snapshot(event: &crate::protocol_v2::CriticalEvent) -> Option<(u64, i32, i32)> {
    match *event {
        crate::protocol_v2::CriticalEvent::Button {
            motion_sequence,
            x,
            y,
            ..
        }
        | crate::protocol_v2::CriticalEvent::Scroll {
            motion_sequence,
            x,
            y,
            ..
        } => Some((motion_sequence, x, y)),
        crate::protocol_v2::CriticalEvent::Key { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_ports::fake::FakeInjector;
    use crate::protocol_v2::{CriticalButton, CriticalEvent, MotionFrame};
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
                    layout_revision: 1,
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

    fn motion(sequence: u64, required_reliable_sequence: u64, x: i32, y: i32) -> MotionFrame {
        MotionFrame {
            session_id: session(),
            display_id: "mac-main".into(),
            layout_revision: 1,
            sequence,
            required_reliable_sequence,
            x,
            y,
        }
    }

    #[test]
    fn a29_maps_retina_negative_origin_and_rejects_stale_or_out_of_range_layout() {
        let authenticated = peer(10);
        let layout = ReceiverDisplayLayout {
            display_id: "mac-main".into(),
            layout_revision: 1,
            logical_width: 1440,
            logical_height: 900,
            native_x: -2560,
            native_y: -1600,
            native_width: 2560,
            native_height: 1600,
        };
        let mut runtime = ReceiverSessionRuntime::new_with_display_layouts(
            boot(2),
            FakeInjector::default(),
            vec![layout.clone()],
        );
        activate(&mut runtime, &authenticated);

        assert_eq!(
            runtime.handle_motion_at(motion(1, 0, 1439, 899), &authenticated, 1),
            Ok(MotionDisposition::Applied)
        );
        assert_eq!(
            runtime.injector().events.last(),
            Some(&InputCommand::MouseMove {
                x: -1,
                y: -1,
                drag_button: None,
            })
        );

        let event_count = runtime.injector().events.len();
        let mut stale = motion(2, 0, 100, 100);
        stale.layout_revision = 2;
        assert!(runtime.handle_motion_at(stale, &authenticated, 2).is_err());
        assert!(runtime
            .handle_motion_at(motion(2, 0, 1440, 100), &authenticated, 3)
            .is_err());
        assert_eq!(runtime.injector().events.len(), event_count);

        let invalid_click = CriticalFrame {
            session_id: session(),
            sequence: 1,
            event: CriticalEvent::Button {
                button: CriticalButton::Left,
                down: true,
                x: 1440,
                y: 100,
                motion_sequence: 1,
            },
        };
        assert!(runtime
            .handle_input_at(&invalid_click, &authenticated, 4)
            .is_err());
        assert_eq!(runtime.injector().events.len(), event_count);
        let valid_key = CriticalFrame {
            sequence: 1,
            event: CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
            ..invalid_click
        };
        assert!(runtime
            .handle_input_at(&valid_key, &authenticated, 5)
            .is_ok());

        let changed = ReceiverDisplayLayout {
            layout_revision: 2,
            ..layout
        };
        assert_eq!(runtime.update_display_layouts(vec![changed]), Ok(true));
        assert!(!runtime.health().active);
        assert_eq!(
            runtime.health().last_fault,
            Some(SessionFault::LayoutChanged)
        );
    }

    #[test]
    fn a30_a31_modifier_mapping_defaults_literal_and_freezes_at_key_down() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);

        let key = |sequence, key_code, scan_code, down| CriticalFrame {
            session_id: session(),
            sequence,
            event: CriticalEvent::Key {
                key_code,
                scan_code,
                extended: false,
                down,
            },
        };
        runtime
            .handle_input_at(&key(1, 0xA2, 29, true), &authenticated, 1)
            .unwrap();
        runtime
            .handle_input_at(&key(2, 0x43, 46, true), &authenticated, 2)
            .unwrap();
        runtime
            .handle_input_at(&key(3, 0x43, 46, false), &authenticated, 3)
            .unwrap();
        runtime
            .handle_input_at(&key(4, 0xA2, 29, false), &authenticated, 4)
            .unwrap();
        assert_eq!(
            &runtime.injector().events[..4],
            &[
                InputCommand::Key {
                    key_code: 0xA2,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 0x43,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 0x43,
                    down: false,
                },
                InputCommand::Key {
                    key_code: 0xA2,
                    down: false,
                },
            ]
        );

        // The Windows key remains Command on macOS, so Windows-side Win+C/V
        // arrives as the native copy/paste chord without changing Ctrl+C.
        for (sequence, key_code, scan_code, down) in [
            (5, 0x5B, 91, true),
            (6, 0x43, 46, true),
            (7, 0x43, 46, false),
            (8, 0x56, 47, true),
            (9, 0x56, 47, false),
            (10, 0x5B, 91, false),
        ] {
            runtime
                .handle_input_at(
                    &key(sequence, key_code, scan_code, down),
                    &authenticated,
                    sequence,
                )
                .unwrap();
        }
        assert_eq!(
            &runtime.injector().events[4..10],
            &[
                InputCommand::Key {
                    key_code: 0x5B,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 0x43,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 0x43,
                    down: false,
                },
                InputCommand::Key {
                    key_code: 0x56,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 0x56,
                    down: false,
                },
                InputCommand::Key {
                    key_code: 0x5B,
                    down: false,
                },
            ]
        );

        runtime.update_modifier_mapping(true, "meta", "same", "control");
        runtime
            .handle_input_at(&key(11, 0xA3, 29, true), &authenticated, 11)
            .unwrap();
        runtime.update_modifier_mapping(false, "same", "same", "same");
        runtime
            .handle_input_at(&key(12, 0xA3, 29, false), &authenticated, 12)
            .unwrap();
        assert_eq!(
            &runtime.injector().events[10..],
            &[
                InputCommand::Key {
                    key_code: 0x5C,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 0x5C,
                    down: false,
                },
            ]
        );
    }

    #[test]
    fn future_motion_is_latest_wins_and_waits_for_reliable_input() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);

        assert_eq!(
            runtime
                .handle_motion_at(motion(1, 1, 100, 100), &authenticated, 10)
                .unwrap(),
            MotionDisposition::Buffered
        );
        assert_eq!(
            runtime
                .handle_motion_at(motion(2, 1, 200, 220), &authenticated, 11)
                .unwrap(),
            MotionDisposition::Buffered
        );
        assert!(runtime.injector().events.is_empty());

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
                12,
            )
            .unwrap();
        assert_eq!(
            runtime.injector().events,
            vec![
                InputCommand::Key {
                    key_code: 65,
                    down: true,
                },
                InputCommand::MouseMove {
                    x: 200,
                    y: 220,
                    drag_button: None,
                },
            ]
        );
        assert_eq!(
            runtime
                .handle_motion_at(motion(1, 0, 1, 1), &authenticated, 13)
                .unwrap(),
            MotionDisposition::Stale
        );
    }

    #[test]
    fn reliable_button_snapshot_precedes_newer_drag_motion() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        runtime
            .handle_motion_at(motion(2, 1, 300, 330), &authenticated, 1)
            .unwrap();

        runtime
            .handle_input_at(
                &CriticalFrame {
                    session_id: session(),
                    sequence: 1,
                    event: CriticalEvent::Button {
                        button: CriticalButton::Left,
                        down: true,
                        x: 120,
                        y: 140,
                        motion_sequence: 1,
                    },
                },
                &authenticated,
                2,
            )
            .unwrap();
        assert_eq!(
            runtime.injector().events,
            vec![
                InputCommand::MouseButton {
                    button: MouseButton::Left,
                    down: true,
                    x: 120,
                    y: 140,
                },
                InputCommand::MouseMove {
                    x: 300,
                    y: 330,
                    drag_button: Some(MouseButton::Left),
                },
            ]
        );
    }

    #[test]
    fn motion_rejects_other_connection_and_is_cleared_on_end() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        assert_eq!(
            runtime.handle_motion_at(motion(1, 0, 20, 30), &peer(11), 1),
            Err(SessionRuntimeError::WrongConnection)
        );
        runtime
            .handle_motion_at(motion(2, 1, 40, 50), &authenticated, 2)
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
        assert!(runtime.injector().events.is_empty());
        assert!(runtime
            .handle_motion_at(motion(3, 0, 60, 70), &authenticated, 3)
            .is_err());
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
    fn a06_return_releases_hotkey_prefix_modifiers() {
        let authenticated = peer(10);
        let mut runtime = ReceiverSessionRuntime::new(boot(2), FakeInjector::default());
        activate(&mut runtime, &authenticated);
        for (sequence, key_code, scan_code) in [(1, 0x11, 0x1d), (2, 0x12, 0x38)] {
            runtime
                .handle_input(
                    &CriticalFrame {
                        session_id: session(),
                        sequence,
                        event: CriticalEvent::Key {
                            key_code,
                            scan_code,
                            extended: false,
                            down: true,
                        },
                    },
                    &authenticated,
                )
                .unwrap();
        }

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
                    key_code: 0x11,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 0x12,
                    down: true,
                },
                InputCommand::Key {
                    key_code: 0x12,
                    down: false,
                },
                InputCommand::Key {
                    key_code: 0x11,
                    down: false,
                },
            ]
        );
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
                    layout_revision: 1,
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
