use crate::routing::LocalOverride;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Mutex,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlHotkeyAction {
    GoMac,
    GoLocal,
    EmergencyLocal,
}

impl ControlHotkeyAction {
    fn bit(self) -> u8 {
        match self {
            Self::GoMac => 1,
            Self::GoLocal => 1 << 1,
            Self::EmergencyLocal => 1 << 2,
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::GoMac => 1,
            Self::GoLocal => 2,
            Self::EmergencyLocal => 3,
        }
    }
}

#[derive(Default)]
pub struct HotkeyDeduper(AtomicU8);

impl HotkeyDeduper {
    pub fn press(&self, action: ControlHotkeyAction) -> bool {
        self.0.fetch_or(action.bit(), Ordering::AcqRel) & action.bit() == 0
    }

    pub fn release(&self, action: ControlHotkeyAction) {
        self.0.fetch_and(!action.bit(), Ordering::AcqRel);
    }
}

#[derive(Default)]
pub struct ControlActionSlot(Mutex<Option<ControlHotkeyAction>>);

impl ControlActionSlot {
    pub fn offer(&self, action: ControlHotkeyAction) -> bool {
        let Ok(mut pending) = self.0.try_lock() else {
            return false;
        };
        if pending
            .as_ref()
            .map_or(true, |current| action.priority() >= current.priority())
        {
            *pending = Some(action);
        }
        true
    }

    pub fn take(&self) -> Option<ControlHotkeyAction> {
        self.0
            .try_lock()
            .ok()
            .and_then(|mut pending| pending.take())
    }
}

pub fn dispatch_control_hotkey(
    deduper: &HotkeyDeduper,
    pending: &ControlActionSlot,
    local_override: &LocalOverride,
    action: ControlHotkeyAction,
    pressed: bool,
) -> bool {
    if !pressed {
        deduper.release(action);
        return false;
    }
    if !deduper.press(action) {
        return false;
    }
    if matches!(
        action,
        ControlHotkeyAction::GoLocal | ControlHotkeyAction::EmergencyLocal
    ) {
        local_override.request_local();
    }
    pending.offer(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control_ports::fake::{FakeCapture, FakeFocus},
        routing::{RouteEffect, Router},
    };

    fn active_local_override() -> std::sync::Arc<LocalOverride> {
        let mut router = Router::default();
        let local = router.local_override();
        let mut capture = FakeCapture::default();
        let request = match router.go_remote("mac", 0, &mut capture).unwrap()[0] {
            RouteEffect::Prepare { request, .. } => request,
            _ => panic!("prepare"),
        };
        router.ready(request, 7).unwrap();
        let mut focus = FakeFocus::default();
        router.advance(1, true, &mut capture, &mut focus).unwrap();
        router.commit_ack(request, 7, 2, &mut capture).unwrap();
        assert!(!local.is_local());
        local
    }

    #[test]
    fn a05_system_and_hook_press_dispatch_once_until_release() {
        let deduper = HotkeyDeduper::default();
        let pending = ControlActionSlot::default();
        let local = LocalOverride::default();

        assert!(dispatch_control_hotkey(
            &deduper,
            &pending,
            &local,
            ControlHotkeyAction::GoMac,
            true,
        ));
        assert!(!dispatch_control_hotkey(
            &deduper,
            &pending,
            &local,
            ControlHotkeyAction::GoMac,
            true,
        ));
        assert_eq!(pending.take(), Some(ControlHotkeyAction::GoMac));
        dispatch_control_hotkey(
            &deduper,
            &pending,
            &local,
            ControlHotkeyAction::GoMac,
            false,
        );
        assert!(dispatch_control_hotkey(
            &deduper,
            &pending,
            &local,
            ControlHotkeyAction::GoMac,
            true,
        ));
    }

    #[test]
    fn return_actions_set_local_gate_before_queue_and_have_priority() {
        let deduper = HotkeyDeduper::default();
        let pending = ControlActionSlot::default();
        let local = active_local_override();
        assert!(pending.offer(ControlHotkeyAction::GoMac));
        assert!(dispatch_control_hotkey(
            &deduper,
            &pending,
            &local,
            ControlHotkeyAction::EmergencyLocal,
            true,
        ));
        assert!(local.is_local());
        assert_eq!(pending.take(), Some(ControlHotkeyAction::EmergencyLocal));
    }
}
