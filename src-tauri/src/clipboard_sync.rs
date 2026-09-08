use crate::protocol_v2::{BootId, ClipboardImageOperation, ClipboardOperationId};

pub(crate) use crate::protocol_v2::{clipboard_text_digest, ClipboardTextOperation};

const MAX_PEER_ID_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct OperationOrder {
    lamport: u64,
    origin_peer: String,
    operation_id: ClipboardOperationId,
}

impl From<&ClipboardTextOperation> for OperationOrder {
    fn from(operation: &ClipboardTextOperation) -> Self {
        Self {
            lamport: operation.lamport,
            origin_peer: operation.origin_peer.clone(),
            operation_id: operation.operation_id,
        }
    }
}

impl From<&ClipboardImageOperation> for OperationOrder {
    fn from(operation: &ClipboardImageOperation) -> Self {
        Self {
            lamport: operation.lamport,
            origin_peer: operation.origin_peer.clone(),
            operation_id: operation.operation_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LocalClipboardDecision {
    Send(ClipboardTextOperation),
    Echo,
    Unchanged,
    Oversized { bytes: usize, limit: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LocalImageDecision {
    Send(ClipboardImageOperation),
    Echo,
    Unchanged,
    Oversized { bytes: usize, limit: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteClipboardDecision {
    Apply,
    IgnoreDuplicate,
    IgnoreStale,
    RejectInvalid,
}

#[derive(Clone, Debug)]
pub(crate) struct ClipboardSyncEngine {
    local_peer: String,
    boot_id: BootId,
    last_local_revision: u64,
    local_sequence: u64,
    lamport: u64,
    text_limit: usize,
    current_order: Option<OperationOrder>,
    current_digest: Option<[u8; 32]>,
    applied_remote_echo: Option<(u64, [u8; 32], ClipboardOperationId)>,
}

impl ClipboardSyncEngine {
    pub(crate) fn new(
        local_peer: String,
        boot_id: BootId,
        baseline_revision: u64,
        text_limit: usize,
    ) -> Result<Self, &'static str> {
        if local_peer.trim().is_empty() || local_peer.len() > MAX_PEER_ID_BYTES {
            return Err("invalid local peer id");
        }
        if text_limit == 0 {
            return Err("text limit must be positive");
        }
        Ok(Self {
            local_peer,
            boot_id,
            last_local_revision: baseline_revision,
            local_sequence: 0,
            lamport: 0,
            text_limit,
            current_order: None,
            current_digest: None,
            applied_remote_echo: None,
        })
    }

    pub(crate) fn observe_local_text(
        &mut self,
        system_revision: u64,
        text: String,
    ) -> LocalClipboardDecision {
        let digest = clipboard_text_digest(text.as_bytes());
        if self
            .applied_remote_echo
            .as_ref()
            .map(|(revision, expected, _)| *revision == system_revision && *expected == digest)
            .unwrap_or(false)
        {
            self.last_local_revision = system_revision;
            self.applied_remote_echo = None;
            return LocalClipboardDecision::Echo;
        }
        if system_revision == self.last_local_revision {
            return LocalClipboardDecision::Unchanged;
        }
        self.last_local_revision = system_revision;
        self.applied_remote_echo = None;
        self.new_local_operation(system_revision, text, digest)
    }

    pub(crate) fn manual_resend_text(
        &mut self,
        system_revision: u64,
        text: String,
    ) -> LocalClipboardDecision {
        self.last_local_revision = system_revision;
        self.applied_remote_echo = None;
        let digest = clipboard_text_digest(text.as_bytes());
        self.new_local_operation(system_revision, text, digest)
    }

    pub(crate) fn observe_local_image(
        &mut self,
        system_revision: u64,
        width: u32,
        height: u32,
        rgba_base64: String,
        encoded_limit: usize,
    ) -> LocalImageDecision {
        let digest = clipboard_text_digest(rgba_base64.as_bytes());
        if self
            .applied_remote_echo
            .as_ref()
            .map(|(revision, expected, _)| *revision == system_revision && *expected == digest)
            .unwrap_or(false)
        {
            self.last_local_revision = system_revision;
            self.applied_remote_echo = None;
            return LocalImageDecision::Echo;
        }
        if system_revision == self.last_local_revision {
            return LocalImageDecision::Unchanged;
        }
        self.last_local_revision = system_revision;
        self.applied_remote_echo = None;
        self.new_local_image_operation(
            system_revision,
            width,
            height,
            rgba_base64,
            encoded_limit,
            digest,
        )
    }

    pub(crate) fn manual_resend_image(
        &mut self,
        system_revision: u64,
        width: u32,
        height: u32,
        rgba_base64: String,
        encoded_limit: usize,
    ) -> LocalImageDecision {
        self.applied_remote_echo = None;
        self.last_local_revision = system_revision;
        let digest = clipboard_text_digest(rgba_base64.as_bytes());
        self.new_local_image_operation(
            system_revision,
            width,
            height,
            rgba_base64,
            encoded_limit,
            digest,
        )
    }

    fn new_local_image_operation(
        &mut self,
        system_revision: u64,
        width: u32,
        height: u32,
        rgba_base64: String,
        encoded_limit: usize,
        digest: [u8; 32],
    ) -> LocalImageDecision {
        if rgba_base64.len() > encoded_limit {
            return LocalImageDecision::Oversized {
                bytes: rgba_base64.len(),
                limit: encoded_limit,
            };
        }
        self.local_sequence = self.local_sequence.saturating_add(1);
        self.lamport = self.lamport.saturating_add(1);
        let operation = ClipboardImageOperation {
            operation_id: ClipboardOperationId {
                boot_id: self.boot_id,
                local_sequence: self.local_sequence,
            },
            origin_peer: self.local_peer.clone(),
            system_revision,
            lamport: self.lamport,
            digest,
            width,
            height,
            rgba_base64,
        };
        self.current_order = Some(OperationOrder::from(&operation));
        self.current_digest = Some(operation.digest);
        LocalImageDecision::Send(operation)
    }

    fn new_local_operation(
        &mut self,
        system_revision: u64,
        text: String,
        digest: [u8; 32],
    ) -> LocalClipboardDecision {
        if text.len() > self.text_limit {
            return LocalClipboardDecision::Oversized {
                bytes: text.len(),
                limit: self.text_limit,
            };
        }
        self.local_sequence = self.local_sequence.saturating_add(1);
        self.lamport = self.lamport.saturating_add(1);
        let operation = ClipboardTextOperation {
            operation_id: ClipboardOperationId {
                boot_id: self.boot_id,
                local_sequence: self.local_sequence,
            },
            origin_peer: self.local_peer.clone(),
            system_revision,
            lamport: self.lamport,
            digest,
            text,
        };
        self.current_order = Some(OperationOrder::from(&operation));
        self.current_digest = Some(operation.digest);
        LocalClipboardDecision::Send(operation)
    }

    pub(crate) fn consider_remote(
        &mut self,
        operation: &ClipboardTextOperation,
    ) -> RemoteClipboardDecision {
        if operation.origin_peer.trim().is_empty()
            || operation.origin_peer.len() > MAX_PEER_ID_BYTES
            || operation.operation_id.local_sequence == 0
            || operation.lamport == 0
            || operation.text.is_empty()
            || operation.text.len() > self.text_limit
            || operation.digest != clipboard_text_digest(operation.text.as_bytes())
        {
            return RemoteClipboardDecision::RejectInvalid;
        }
        self.lamport = self.lamport.max(operation.lamport);
        let incoming = OperationOrder::from(operation);
        match self.current_order.as_ref() {
            Some(current) if &incoming == current => RemoteClipboardDecision::IgnoreDuplicate,
            Some(current) if &incoming < current => RemoteClipboardDecision::IgnoreStale,
            _ => RemoteClipboardDecision::Apply,
        }
    }

    pub(crate) fn consider_remote_image(
        &mut self,
        operation: &ClipboardImageOperation,
    ) -> RemoteClipboardDecision {
        if operation.origin_peer.trim().is_empty()
            || operation.origin_peer.len() > MAX_PEER_ID_BYTES
            || operation.operation_id.local_sequence == 0
            || operation.lamport == 0
            || operation.width == 0
            || operation.height == 0
            || operation.rgba_base64.is_empty()
            || operation.digest != clipboard_text_digest(operation.rgba_base64.as_bytes())
        {
            return RemoteClipboardDecision::RejectInvalid;
        }
        self.lamport = self.lamport.max(operation.lamport);
        let incoming = OperationOrder::from(operation);
        match self.current_order.as_ref() {
            Some(current) if &incoming == current => RemoteClipboardDecision::IgnoreDuplicate,
            Some(current) if &incoming < current => RemoteClipboardDecision::IgnoreStale,
            _ => RemoteClipboardDecision::Apply,
        }
    }

    #[cfg(test)]
    pub(crate) fn commit_remote(
        &mut self,
        operation: &ClipboardTextOperation,
        applied_system_revision: u64,
    ) {
        self.commit_remote_parts(
            operation.lamport,
            operation.origin_peer.clone(),
            operation.operation_id,
            operation.digest,
            applied_system_revision,
        );
    }

    #[cfg(test)]
    pub(crate) fn commit_remote_image(
        &mut self,
        operation: &ClipboardImageOperation,
        applied_system_revision: u64,
    ) {
        self.commit_remote_parts(
            operation.lamport,
            operation.origin_peer.clone(),
            operation.operation_id,
            operation.digest,
            applied_system_revision,
        );
    }

    pub(crate) fn commit_remote_parts(
        &mut self,
        lamport: u64,
        origin_peer: String,
        operation_id: ClipboardOperationId,
        digest: [u8; 32],
        applied_system_revision: u64,
    ) {
        self.current_order = Some(OperationOrder {
            lamport,
            origin_peer,
            operation_id,
        });
        self.current_digest = Some(digest);
        self.applied_remote_echo = Some((applied_system_revision, digest, operation_id));
    }

    #[cfg(test)]
    pub(crate) fn current_digest(&self) -> Option<[u8; 32]> {
        self.current_digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol_v2::BootId;

    fn engine(peer: &str, boot: u8, baseline_revision: u64) -> ClipboardSyncEngine {
        ClipboardSyncEngine::new(
            peer.into(),
            BootId([boot; 16]),
            baseline_revision,
            1024 * 1024,
        )
        .expect("valid engine")
    }

    #[test]
    fn a32_text_round_trip_preserves_unicode_newlines_and_long_utf8() {
        let text = format!("中文🙂\r\nline two\n{}", "长文本".repeat(80_000));
        let mut sender = engine("windows", 1, 7);
        let LocalClipboardDecision::Send(operation) = sender.observe_local_text(8, text.clone())
        else {
            panic!("new text must produce an operation")
        };

        let encoded = operation.encode().expect("encode clipboard operation");
        let decoded = ClipboardTextOperation::decode(&encoded).expect("decode clipboard operation");
        assert_eq!(decoded.text, text);
        assert_eq!(
            decoded.digest,
            clipboard_text_digest(decoded.text.as_bytes())
        );
    }

    #[test]
    fn a32_oversized_text_is_visible_and_never_truncated() {
        let mut sender = ClipboardSyncEngine::new("windows".into(), BootId([1; 16]), 1, 8)
            .expect("valid engine");
        assert_eq!(
            sender.observe_local_text(2, "中文abc".into()),
            LocalClipboardDecision::Oversized {
                bytes: "中文abc".len(),
                limit: 8,
            }
        );
    }

    #[test]
    fn a34_remote_echo_is_suppressed_but_immediate_new_copy_is_sent() {
        let mut windows = engine("windows", 1, 10);
        let mut mac = engine("mac", 2, 20);
        let LocalClipboardDecision::Send(remote) =
            windows.observe_local_text(11, "remote A".into())
        else {
            panic!("expected operation")
        };
        assert_eq!(mac.consider_remote(&remote), RemoteClipboardDecision::Apply);
        mac.commit_remote(&remote, 21);

        assert_eq!(
            mac.observe_local_text(21, "remote A".into()),
            LocalClipboardDecision::Echo
        );
        assert!(matches!(
            mac.observe_local_text(22, "local B".into()),
            LocalClipboardDecision::Send(operation) if operation.text == "local B"
        ));
    }

    #[test]
    fn a35_concurrent_out_of_order_and_duplicate_operations_converge() {
        let mut a = engine("a-peer", 1, 1);
        let mut z = engine("z-peer", 2, 1);
        let LocalClipboardDecision::Send(a_copy) = a.observe_local_text(2, "A".into()) else {
            panic!("A operation")
        };
        let LocalClipboardDecision::Send(z_copy) = z.observe_local_text(2, "Z".into()) else {
            panic!("Z operation")
        };

        assert_eq!(a.consider_remote(&z_copy), RemoteClipboardDecision::Apply);
        a.commit_remote(&z_copy, 3);
        assert_eq!(
            z.consider_remote(&a_copy),
            RemoteClipboardDecision::IgnoreStale
        );
        assert_eq!(
            a.consider_remote(&z_copy),
            RemoteClipboardDecision::IgnoreDuplicate
        );
        assert_eq!(a.current_digest(), Some(z_copy.digest));
        assert_eq!(z.current_digest(), Some(z_copy.digest));
    }

    #[test]
    fn a36_reconnect_primes_revision_and_manual_resend_is_explicit() {
        let mut restarted = engine("mac", 3, 50);
        assert_eq!(
            restarted.observe_local_text(50, "old clipboard".into()),
            LocalClipboardDecision::Unchanged
        );
        assert!(matches!(
            restarted.manual_resend_text(50, "old clipboard".into()),
            LocalClipboardDecision::Send(operation) if operation.text == "old clipboard"
        ));
    }

    #[test]
    fn a38_image_operation_round_trips_and_uses_the_same_echo_rule() {
        let mut sender = engine("windows", 1, 1);
        let LocalImageDecision::Send(operation) =
            sender.observe_local_image(2, 1, 1, "AAAAAA==".into(), 32 * 1024 * 1024)
        else {
            panic!("image operation")
        };
        let encoded = operation.encode().unwrap();
        let crate::protocol_v2::ClipboardBulkOperation::Image(decoded) =
            crate::protocol_v2::decode_clipboard_bulk(&encoded).unwrap()
        else {
            panic!("decoded image operation")
        };
        assert_eq!(decoded, operation);

        let mut receiver = engine("mac", 2, 10);
        assert_eq!(
            receiver.consider_remote_image(&decoded),
            RemoteClipboardDecision::Apply
        );
        receiver.commit_remote_image(&decoded, 11);
        assert_eq!(
            receiver.observe_local_image(11, 1, 1, decoded.rgba_base64, 32 * 1024 * 1024),
            LocalImageDecision::Echo
        );
    }
}
