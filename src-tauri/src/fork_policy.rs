//! Compile-time policy for the ordinary-user, local preview fork.
//! This is deliberately not controlled by IPC or imported user configuration.
pub(crate) const PRIVILEGED_FEATURES_ENABLED: bool = false;

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
    }
}
