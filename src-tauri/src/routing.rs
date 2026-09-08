//! Pure coordinator. The caller must validate connection identity before delivering
//! Ready/CommitAck. This module is not an IPC endpoint or an authentication layer.
use crate::control_ports::{CapturePort, FocusPort, PortError};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

const HANDOFF_TIMEOUT_MS: u64 = 2500;
const KEYS_RELEASE_TIMEOUT_MS: u64 = 1500;

/// One atomic contains both the cancellation generation and local/remote bit.
/// A late ACK cannot clear an emergency request via a load/store race.
#[derive(Debug)]
pub struct LocalOverride(AtomicU64);

impl Default for LocalOverride {
    fn default() -> Self {
        Self(AtomicU64::new(1))
    }
}

impl LocalOverride {
    pub fn is_local(&self) -> bool {
        self.0.load(Ordering::Acquire) & 1 != 0
    }

    /// Independent of the actor, network queues, locks and platform callbacks.
    pub fn request_local(&self) {
        let _ = self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                Some(v.saturating_add(2) | 1)
            });
    }

    fn reserve(&self) -> Result<u64, RouteError> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                v.checked_add(2)
                    .map(|next| next | 1)
                    .filter(|next| *next < u64::MAX)
            })
            .map(|previous| (previous + 2) | 1)
            .map_err(|_| RouteError::Exhausted)
    }

    fn still_preparing(&self, generation: u64) -> bool {
        self.0.load(Ordering::Acquire) == generation
    }

    fn activate(&self, generation: u64) -> bool {
        generation != u64::MAX
            && self
                .0
                .compare_exchange(
                    generation,
                    generation & !1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnReason {
    User,
    Emergency,
    Timeout,
    KeysHeld,
    FocusUnavailable,
    CaptureFailed,
    NetworkFailed,
    QueueFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteError {
    InvalidPeer,
    Busy,
    StaleReply,
    Exhausted,
    Capture(PortError),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteState {
    LocalDesktop,
    LocalGame,
    PreparingRemote {
        peer: String,
        request: u64,
        deadline_ms: u64,
        keys_deadline_ms: u64,
        keys_released_seen: bool,
        session: Option<u128>,
        commit_sent: bool,
    },
    RemoteActive {
        peer: String,
        session: u128,
    },
    ReturningLocal {
        reason: ReturnReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteEffect {
    Prepare { peer: String, request: u64 },
    Commit { request: u64, session: u128 },
    Cancel { request: u64 },
    End { session: u128, reason: ReturnReason },
}

pub struct Router {
    state: RouteState,
    game_mode: bool,
    gate: Arc<LocalOverride>,
    last_return: Option<ReturnReason>,
}

impl Default for Router {
    fn default() -> Self {
        Self {
            state: RouteState::LocalDesktop,
            game_mode: false,
            gate: Arc::new(LocalOverride::default()),
            last_return: None,
        }
    }
}

impl Router {
    pub fn with_gate(gate: Arc<LocalOverride>) -> Self {
        Self {
            state: RouteState::LocalDesktop,
            game_mode: false,
            gate,
            last_return: None,
        }
    }

    pub fn state(&self) -> &RouteState {
        &self.state
    }
    pub fn local_override(&self) -> Arc<LocalOverride> {
        Arc::clone(&self.gate)
    }
    pub fn last_return_reason(&self) -> Option<ReturnReason> {
        self.last_return
    }

    fn local_state(&self) -> RouteState {
        if self.game_mode {
            RouteState::LocalGame
        } else {
            RouteState::LocalDesktop
        }
    }

    pub fn set_game_mode(&mut self, enabled: bool) {
        self.game_mode = enabled;
        if matches!(self.state, RouteState::LocalDesktop | RouteState::LocalGame) {
            self.state = self.local_state();
        }
    }

    pub fn go_remote<C: CapturePort>(
        &mut self,
        peer: &str,
        now_ms: u64,
        capture: &mut C,
    ) -> Result<Vec<RouteEffect>, RouteError> {
        if peer.trim().is_empty() || peer.len() > 256 {
            return Err(RouteError::InvalidPeer);
        }
        match &self.state {
            RouteState::PreparingRemote { peer: current, .. }
            | RouteState::RemoteActive { peer: current, .. } => {
                return if current == peer {
                    Ok(vec![])
                } else {
                    Err(RouteError::Busy)
                };
            }
            RouteState::ReturningLocal { .. } => return Err(RouteError::Busy),
            _ => {}
        }
        let deadline_ms = now_ms
            .checked_add(HANDOFF_TIMEOUT_MS)
            .ok_or(RouteError::Exhausted)?;
        let keys_deadline_ms = now_ms
            .checked_add(KEYS_RELEASE_TIMEOUT_MS)
            .ok_or(RouteError::Exhausted)?;
        let request = self.gate.reserve()?;
        self.state = RouteState::PreparingRemote {
            peer: peer.to_owned(),
            request,
            deadline_ms,
            keys_deadline_ms,
            keys_released_seen: false,
            session: None,
            commit_sent: false,
        };
        if let Err(error) = capture.prepare_capture() {
            self.go_local(ReturnReason::CaptureFailed, capture);
            return Err(RouteError::Capture(error));
        }
        if !self.gate.still_preparing(request) {
            self.go_local(ReturnReason::Emergency, capture);
            return Err(RouteError::Cancelled);
        }
        Ok(vec![RouteEffect::Prepare {
            peer: peer.to_owned(),
            request,
        }])
    }

    pub fn ready(&mut self, request: u64, offered_session: u128) -> Result<(), RouteError> {
        if offered_session == 0 || !self.gate.still_preparing(request) {
            return Err(RouteError::StaleReply);
        }
        let RouteState::PreparingRemote {
            request: expected,
            session,
            ..
        } = &mut self.state
        else {
            return Err(RouteError::StaleReply);
        };
        if request != *expected || session.is_some_and(|s| s != offered_session) {
            return Err(RouteError::StaleReply);
        }
        *session = Some(offered_session);
        Ok(())
    }

    /// Caller samples physical key/button state before processing ACKs. The
    /// actor alone owns state; hooks only consult LocalOverride and queue events.
    pub fn advance<C: CapturePort, F: FocusPort>(
        &mut self,
        now_ms: u64,
        keys_released: bool,
        capture: &mut C,
        focus: &mut F,
    ) -> Result<Vec<RouteEffect>, RouteError> {
        let RouteState::PreparingRemote {
            request,
            deadline_ms,
            keys_deadline_ms,
            keys_released_seen,
            session,
            commit_sent,
            ..
        } = self.state.clone()
        else {
            if matches!(self.state, RouteState::RemoteActive { .. }) && self.gate.is_local() {
                return Ok(self.go_local(ReturnReason::Emergency, capture));
            }
            return Ok(vec![]);
        };
        let reason = if !self.gate.still_preparing(request) {
            Some(ReturnReason::Emergency)
        } else if now_ms >= deadline_ms {
            Some(ReturnReason::Timeout)
        } else if (!keys_released && commit_sent)
            || ((!keys_released || !keys_released_seen) && now_ms >= keys_deadline_ms)
        {
            Some(ReturnReason::KeysHeld)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Ok(self.go_local(reason, capture));
        }
        if let RouteState::PreparingRemote {
            keys_released_seen, ..
        } = &mut self.state
        {
            *keys_released_seen = keys_released;
        }
        if commit_sent || !keys_released {
            return Ok(vec![]);
        }
        let Some(session) = session else {
            return Ok(vec![]);
        };
        if focus.prepare_focus().is_err() {
            return Ok(self.go_local(ReturnReason::FocusUnavailable, capture));
        }
        if !self.gate.still_preparing(request) {
            return Ok(self.go_local(ReturnReason::Emergency, capture));
        }
        if let RouteState::PreparingRemote { commit_sent, .. } = &mut self.state {
            *commit_sent = true;
        }
        Ok(vec![RouteEffect::Commit { request, session }])
    }

    /// On error the transport adapter must close/abort the attempted authenticated
    /// session. It must never retain remote control after a failed activation.
    pub fn commit_ack<C: CapturePort>(
        &mut self,
        request: u64,
        session: u128,
        now_ms: u64,
        capture: &mut C,
    ) -> Result<(), RouteError> {
        let RouteState::PreparingRemote {
            peer,
            request: expected,
            session: offered,
            deadline_ms,
            commit_sent,
            ..
        } = self.state.clone()
        else {
            return Err(RouteError::StaleReply);
        };
        if expected != request || offered != Some(session) || !commit_sent {
            return Err(RouteError::StaleReply);
        }
        if now_ms >= deadline_ms || !self.gate.still_preparing(request) {
            self.go_local(
                if now_ms >= deadline_ms {
                    ReturnReason::Timeout
                } else {
                    ReturnReason::Emergency
                },
                capture,
            );
            return Err(RouteError::Cancelled);
        }
        // The gate is still local while the platform prepares active capture.
        // A racing emergency changes its generation, preventing activation.
        if let Err(error) = capture.activate_capture() {
            self.go_local(ReturnReason::CaptureFailed, capture);
            return Err(RouteError::Capture(error));
        }
        if !self.gate.activate(request) {
            self.go_local(ReturnReason::Emergency, capture);
            return Err(RouteError::Cancelled);
        }
        self.state = RouteState::RemoteActive { peer, session };
        Ok(())
    }

    /// Local recovery happens before returning effects for asynchronous delivery.
    pub fn go_local<C: CapturePort>(
        &mut self,
        reason: ReturnReason,
        capture: &mut C,
    ) -> Vec<RouteEffect> {
        self.gate.request_local();
        let old = std::mem::replace(&mut self.state, RouteState::ReturningLocal { reason });
        if !matches!(old, RouteState::LocalDesktop | RouteState::LocalGame) {
            capture.request_local_restore();
        }
        self.last_return = Some(reason);
        self.state = self.local_state();
        match old {
            RouteState::RemoteActive { session, .. }
            | RouteState::PreparingRemote {
                session: Some(session),
                ..
            } => vec![RouteEffect::End { session, reason }],
            RouteState::PreparingRemote { request, .. } => vec![RouteEffect::Cancel { request }],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_ports::fake::{FakeCapture, FakeFocus};

    #[test]
    fn partial_prepare_failure_requests_cleanup() {
        let mut router = Router::default();
        let mut capture = FakeCapture {
            failure: Some(PortError::Unavailable),
            ..Default::default()
        };
        assert!(router.go_remote("mac", 0, &mut capture).is_err());
        assert_eq!(capture.restored, 1);
    }

    #[test]
    fn first_observed_key_release_after_deadline_cannot_commit() {
        let (mut r, mut c, request) = preparing();
        r.ready(request, 7).unwrap();
        let effects = r
            .advance(1600, true, &mut c, &mut FakeFocus::default())
            .unwrap();
        assert!(!effects
            .iter()
            .any(|e| matches!(e, RouteEffect::Commit { .. })));
        assert_eq!(r.last_return_reason(), Some(ReturnReason::KeysHeld));
    }

    #[test]
    fn emergency_during_native_activation_wins_over_compare_exchange() {
        struct RacingCapture {
            gate: Arc<LocalOverride>,
            restored: bool,
        }
        impl CapturePort for RacingCapture {
            fn prepare_capture(&mut self) -> Result<(), PortError> {
                Ok(())
            }
            fn activate_capture(&mut self) -> Result<(), PortError> {
                self.gate.request_local();
                Ok(())
            }
            fn request_local_restore(&mut self) {
                self.restored = true;
            }
        }
        let (mut r, mut c, request) = preparing();
        r.ready(request, 7).unwrap();
        r.advance(1, true, &mut c, &mut FakeFocus::default())
            .unwrap();
        let mut c = RacingCapture {
            gate: r.local_override(),
            restored: false,
        };
        assert!(r.commit_ack(request, 7, 2, &mut c).is_err());
        assert!(c.restored && c.gate.is_local());
    }

    #[test]
    fn exhausted_generation_never_rearms() {
        let gate = LocalOverride(AtomicU64::new(u64::MAX - 2));
        gate.request_local();
        assert!(gate.reserve().is_err());
        assert!(!gate.activate(u64::MAX));
        assert!(gate.is_local());
    }

    fn preparing() -> (Router, FakeCapture, u64) {
        let mut router = Router::default();
        let mut capture = FakeCapture::default();
        let effects = router.go_remote("mac", 0, &mut capture).unwrap();
        let RouteEffect::Prepare { request, .. } = effects[0] else {
            panic!("prepare")
        };
        (router, capture, request)
    }

    fn active() -> (Router, FakeCapture, u64) {
        let (mut router, mut capture, request) = preparing();
        router.ready(request, 7).unwrap();
        let mut focus = FakeFocus::default();
        assert_eq!(
            router.advance(1, true, &mut capture, &mut focus).unwrap(),
            vec![RouteEffect::Commit {
                request,
                session: 7
            }]
        );
        router.commit_ack(request, 7, 2, &mut capture).unwrap();
        (router, capture, request)
    }

    #[test]
    fn a01_same_target_is_idempotent_in_preparation_and_remote() {
        let (mut r, mut c, _) = preparing();
        assert!(r.go_remote("mac", 10, &mut c).unwrap().is_empty());
        assert_eq!(c.prepared, 1);
        let (mut r, mut c, _) = active();
        assert!(r.go_remote("mac", 10, &mut c).unwrap().is_empty());
        assert_eq!(c.activated, 1);
        assert!(r.go_remote("other", 10, &mut c).is_err());
    }

    #[test]
    fn a02_return_restores_before_caller_can_send_end() {
        let (mut r, mut c, _) = active();
        let effects = r.go_local(ReturnReason::User, &mut c);
        assert!(r.local_override().is_local());
        assert_eq!(c.restored, 1);
        assert_eq!(r.state(), &RouteState::LocalDesktop);
        assert_eq!(
            effects,
            vec![RouteEffect::End {
                session: 7,
                reason: ReturnReason::User
            }]
        );
        // Discard the effects: even a permanently blocked transport cannot undo restoration.
    }

    #[test]
    fn a03_cancelled_request_rejects_late_ready_and_ack() {
        let (mut r, mut c, request) = preparing();
        r.go_local(ReturnReason::User, &mut c);
        assert!(r.ready(request, 7).is_err());
        assert!(r.commit_ack(request, 7, 1, &mut c).is_err());
        assert_eq!(c.activated, 0);
        assert!(r.local_override().is_local());
    }

    #[test]
    fn a04_held_keys_timeout_without_committing_or_swallowing() {
        let (mut r, mut c, request) = preparing();
        r.ready(request, 7).unwrap();
        let mut f = FakeFocus::default();
        assert!(r.advance(1499, false, &mut c, &mut f).unwrap().is_empty());
        r.advance(1500, false, &mut c, &mut f).unwrap();
        assert_eq!(r.state(), &RouteState::LocalDesktop);
        assert_eq!((f.attempts, c.activated), (0, 0));
        assert_eq!(r.last_return_reason(), Some(ReturnReason::KeysHeld));
    }

    #[test]
    fn a07_emergency_does_not_need_coordinator_lock() {
        let (r, _, _) = active();
        let gate = r.local_override();
        let actor = std::sync::Mutex::new(r);
        let _blocked_actor = actor.lock().unwrap();
        std::thread::spawn(move || {
            gate.request_local();
            assert!(gate.is_local());
        })
        .join()
        .unwrap();
    }

    #[test]
    fn emergency_between_commit_and_ack_cannot_be_cleared_by_ack() {
        let (mut r, mut c, request) = preparing();
        r.ready(request, 7).unwrap();
        r.advance(1, true, &mut c, &mut FakeFocus::default())
            .unwrap();
        r.local_override().request_local();
        assert!(r.commit_ack(request, 7, 2, &mut c).is_err());
        assert!(r.local_override().is_local());
        assert_eq!(c.activated, 0);
    }

    #[test]
    fn wrong_session_early_ack_and_old_request_never_activate() {
        let (mut r, mut c, request) = preparing();
        assert!(r.commit_ack(request, 7, 1, &mut c).is_err());
        r.ready(request, 7).unwrap();
        r.advance(1, true, &mut c, &mut FakeFocus::default())
            .unwrap();
        assert!(r.commit_ack(request, 8, 2, &mut c).is_err());
        assert!(r.commit_ack(request + 1, 7, 2, &mut c).is_err());
        assert_eq!(c.activated, 0);
    }

    #[test]
    fn a09_focus_denial_tries_once_then_returns_local() {
        let (mut r, mut c, request) = preparing();
        r.ready(request, 7).unwrap();
        let mut f = FakeFocus {
            failure: Some(PortError::PermissionDenied),
            ..Default::default()
        };
        r.advance(1, true, &mut c, &mut f).unwrap();
        r.advance(2, true, &mut c, &mut f).unwrap();
        assert_eq!((f.attempts, c.activated, c.restored), (1, 0, 1));
        assert_eq!(r.last_return_reason(), Some(ReturnReason::FocusUnavailable));
    }

    #[test]
    fn returns_to_latest_selected_local_mode() {
        let (mut r, mut c, _) = active();
        r.set_game_mode(true);
        r.go_local(ReturnReason::User, &mut c);
        assert_eq!(r.state(), &RouteState::LocalGame);
        r.set_game_mode(false);
        assert_eq!(r.state(), &RouteState::LocalDesktop);
    }

    #[test]
    fn deadline_and_capture_failure_remain_local() {
        let (mut r, mut c, request) = preparing();
        r.ready(request, 7).unwrap();
        r.advance(1, true, &mut c, &mut FakeFocus::default())
            .unwrap();
        assert!(r.commit_ack(request, 7, 2500, &mut c).is_err());
        assert!(r.local_override().is_local());
        let (mut r, mut c, request) = preparing();
        r.ready(request, 7).unwrap();
        r.advance(1, true, &mut c, &mut FakeFocus::default())
            .unwrap();
        c.failure = Some(PortError::Unavailable);
        assert!(r.commit_ack(request, 7, 2, &mut c).is_err());
        assert!(r.local_override().is_local());
        assert_eq!(c.restored, 1);
    }

    #[test]
    fn invalid_peer_and_clock_overflow_fail_without_side_effects() {
        let mut r = Router::default();
        let mut c = FakeCapture::default();
        assert!(r.go_remote("", 0, &mut c).is_err());
        assert!(r.go_remote("mac", u64::MAX, &mut c).is_err());
        assert_eq!(c.prepared, 0);
        assert!(r.local_override().is_local());
    }

    #[test]
    fn duplicate_ready_cannot_replace_committed_session() {
        let (mut r, mut c, request) = preparing();
        r.ready(request, 7).unwrap();
        r.advance(1, true, &mut c, &mut FakeFocus::default())
            .unwrap();
        assert!(r.ready(request, 8).is_err());
        assert!(r.commit_ack(request, 8, 2, &mut c).is_err());
        r.commit_ack(request, 7, 2, &mut c).unwrap();
        assert_eq!(c.activated, 1);
    }
}
