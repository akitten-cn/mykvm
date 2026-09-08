//! Compile-time policy for the ordinary-user, local preview fork.
//! This is deliberately not controlled by IPC or imported user configuration.
pub(crate) const PRIVILEGED_FEATURES_ENABLED: bool = false;

// Keep V1 disabled permanently. T05/T06 must provide a separate authenticated
// connection/session path, not flip this flag to revive address-based authority.
pub(crate) const LEGACY_LAN_DATA_ENABLED: bool = false;

// Enabled only for the authenticated V2 receiver. V1 remains permanently off.
// T07 provides the pressed-input ledger, stream-close cleanup and lease timeout.
pub(crate) const V2_NATIVE_RECEIVER_ENABLED: bool = true;

// T08-T11 provide bounded motion, layout gating, physical-release checks,
// control hotkeys, the local-game fast path and conservative focus handoff.
// Platform build/runtime evidence remains tracked separately from this gate.
pub(crate) const V2_NATIVE_CONTROLLER_ENABLED: bool = true;

pub(crate) fn require_privileged_features() -> Result<(), String> {
    if PRIVILEGED_FEATURES_ENABLED {
        Ok(())
    } else {
        Err("MyKVM Local 仅使用普通用户会话，不提供提权、系统输入服务或安全桌面控制。".into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn privileged_service_actions_are_rejected() {
        assert!(super::require_privileged_features().is_err());
        assert!(super::V2_NATIVE_RECEIVER_ENABLED);
        assert!(super::V2_NATIVE_CONTROLLER_ENABLED);
    }
}
