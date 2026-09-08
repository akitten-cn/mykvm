use std::{
    collections::HashMap,
    fs,
    net::{SocketAddr, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use quinn::{
    rustls::{
        self,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        crypto::{
            ring::default_provider, verify_tls12_signature, verify_tls13_signature,
            WebPkiSupportedAlgorithms,
        },
        pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
        server::danger::{ClientCertVerified, ClientCertVerifier},
        DigitallySignedStruct, SignatureScheme,
    },
    ClientConfig, Endpoint, ServerConfig,
};
use tokio::sync::mpsc as tokio_mpsc;

use crate::protocol_v2::{
    self, ControlDecoder, ControlFrame, CriticalDecoder, CriticalFrame, MotionFrame,
};

pub const PROTOCOL_VERSION: u16 = 1;

const SERVER_NAME: &str = "mykvm.local";
const MAX_DATAGRAM_BYTES: usize = 16 * 1024;
// Clipboard images are sent as RGBA base64 over streams. The clipboard module
// caps decoded images at 32 MiB, which becomes roughly 43 MiB on the wire.
pub(crate) const MAX_STREAM_BYTES: usize = 48 * 1024 * 1024;
const PORT_SCAN_COUNT: u16 = 64;
const QUIC_WORKER_THREADS: usize = 2;
// Datagram-health fast-fail (concept adopted from PR #22): after this many
// consecutive send/connect failures to a peer, sends short-circuit with an
// error until the retry window elapses, so the input layer releases the
// cursor immediately instead of freezing behind connect timeouts.
const DATAGRAM_FAIL_THRESHOLD: u32 = 3;
const DATAGRAM_RETRY_WINDOW: Duration = Duration::from_secs(3);
const MAX_HEALTH_PEERS: usize = 64;
// Streams (clipboard, files) are handled in spawned tasks; cap the concurrent
// in-flight count so a burst cannot spawn unbounded copies of a 48MB write.
const MAX_CONCURRENT_STREAMS: usize = 8;
const MAX_INBOUND_STREAMS: usize = 8;
const MAX_BULK_MEMORY_BYTES: usize = 128 * 1024 * 1024;
const INBOUND_BULK_RESERVATION_BYTES: usize = 126 * 1024 * 1024;
const MAX_CONTROL_CONNECTIONS: usize = 8;
const CONTROL_QUEUE_FRAMES: usize = 64;
const MAX_CONTROL_FRAMES_PER_SECOND: u32 = 128;
const CONTROL_PREFACE: &[u8; 4] = b"MKC2";
const INPUT_PREFACE: &[u8; 4] = b"MKI2";
const INPUT_QUEUE_FRAMES: usize = 256;
const INPUT_QUEUE_BYTES: usize = 256 * 1024;
const MAX_INPUT_CONNECTIONS: usize = 8;

const ALPN_V2: &[u8] = b"mykvm-local/2";

type DatagramHandler = Arc<dyn Fn(Vec<u8>, AuthenticatedPeer) + Send + Sync + 'static>;
type StreamHandler = Arc<dyn Fn(Vec<u8>, ConnectionPeer) -> bool + Send + Sync + 'static>;
type ControlHandler =
    Arc<dyn Fn(ControlFrame, AuthenticatedPeer) -> Option<ControlFrame> + Send + Sync + 'static>;
type OutboundControlHandler =
    Arc<dyn Fn(ControlFrame) -> Option<ControlFrame> + Send + Sync + 'static>;
type InputHandler = Arc<dyn Fn(CriticalFrame, AuthenticatedPeer) -> bool + Send + Sync + 'static>;
type InputClosedHandler = Arc<dyn Fn(AuthenticatedPeer, String) + Send + Sync + 'static>;

struct BulkMemoryBudget {
    used: AtomicUsize,
    limit: usize,
}

impl BulkMemoryBudget {
    fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            used: AtomicUsize::new(0),
            limit,
        })
    }

    fn reserve(self: &Arc<Self>, bytes: usize) -> Result<BulkMemoryReservation, String> {
        let mut used = self.used.load(Ordering::Acquire);
        loop {
            let Some(next) = used.checked_add(bytes) else {
                return Err("bulk memory budget overflow".into());
            };
            if next > self.limit {
                return Err(format!(
                    "bulk memory budget exceeded: {next} bytes requested, {} bytes available",
                    self.limit.saturating_sub(used)
                ));
            }
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    return Ok(BulkMemoryReservation {
                        budget: Arc::clone(self),
                        bytes,
                    })
                }
                Err(current) => used = current,
            }
        }
    }
}

struct BulkMemoryReservation {
    budget: Arc<BulkMemoryBudget>,
    bytes: usize,
}

impl Drop for BulkMemoryReservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerRole {
    Controller,
    Receiver,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedPeer {
    pub peer_id: String,
    pub certificate: String,
    pub role: PeerRole,
    pub trust_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedPeer {
    pub peer_id: String,
    pub role: PeerRole,
    pub trust_revision: u64,
    pub connection_generation: u64,
    pub remote_addr: SocketAddr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionPeer {
    Authenticated(AuthenticatedPeer),
    Unauthenticated {
        remote_addr: SocketAddr,
        connection_generation: u64,
        presented_certificate: Option<String>,
    },
}

#[derive(Clone, Default)]
pub struct TrustedPeerRegistry(Arc<RwLock<Vec<DecodedTrustedPeer>>>);

#[derive(Clone, Debug)]
struct DecodedTrustedPeer {
    peer_id: String,
    certificate: Vec<u8>,
    role: PeerRole,
    trust_revision: u64,
}

impl TrustedPeerRegistry {
    pub fn new(peers: Vec<TrustedPeer>) -> Result<Self, String> {
        let registry = Self::default();
        registry.replace(peers)?;
        Ok(registry)
    }

    pub fn replace(&self, peers: Vec<TrustedPeer>) -> Result<(), String> {
        let mut decoded = Vec::with_capacity(peers.len());
        for peer in peers {
            if peer.peer_id.trim().is_empty() || peer.trust_revision == 0 {
                return Err("trusted peer requires a non-empty id and revision".into());
            }
            let certificate = BASE64
                .decode(peer.certificate.as_bytes())
                .map_err(|error| {
                    format!("invalid trusted certificate for {}: {error}", peer.peer_id)
                })?;
            if certificate.is_empty() {
                return Err(format!("trusted certificate for {} is empty", peer.peer_id));
            }
            if decoded.iter().any(|existing: &DecodedTrustedPeer| {
                existing.peer_id == peer.peer_id || existing.certificate == certificate
            }) {
                return Err("trusted peer ids and certificates must be unique".into());
            }
            decoded.push(DecodedTrustedPeer {
                peer_id: peer.peer_id,
                certificate,
                role: peer.role,
                trust_revision: peer.trust_revision,
            });
        }
        *self
            .0
            .write()
            .map_err(|_| "trust store lock poisoned".to_string())? = decoded;
        Ok(())
    }

    fn authenticate(
        &self,
        certificate: &[u8],
        remote_addr: SocketAddr,
        connection_generation: u64,
    ) -> Option<AuthenticatedPeer> {
        let peers = self.0.read().ok()?;
        let peer = peers.iter().find(|peer| peer.certificate == certificate)?;
        Some(AuthenticatedPeer {
            peer_id: peer.peer_id.clone(),
            role: peer.role,
            trust_revision: peer.trust_revision,
            connection_generation,
            remote_addr,
        })
    }

    fn control_peer(
        &self,
        peer_id: &str,
        role: PeerRole,
        addr: String,
        protocol_version: u16,
    ) -> Result<ControlPeer, String> {
        let peers = self
            .0
            .read()
            .map_err(|_| "trust store lock poisoned".to_string())?;
        let peer = peers
            .iter()
            .find(|peer| peer.peer_id == peer_id && peer.role == role)
            .ok_or_else(|| format!("peer {peer_id} is not trusted for role {role:?}"))?;
        Ok(ControlPeer {
            peer_id: peer.peer_id.clone(),
            role: peer.role,
            trust_revision: peer.trust_revision,
            endpoint: PeerEndpoint {
                addr,
                public_key: BASE64.encode(&peer.certificate),
                protocol_version,
            },
        })
    }
}

#[derive(Clone, Debug)]
pub struct PeerEndpoint {
    pub addr: String,
    pub public_key: String,
    pub protocol_version: u16,
}

#[derive(Clone, Debug)]
pub struct ControlPeer {
    peer_id: String,
    role: PeerRole,
    trust_revision: u64,
    endpoint: PeerEndpoint,
}

#[derive(Clone)]
pub struct ControlHandle {
    outgoing: tokio_mpsc::Sender<ControlFrame>,
}

struct QueuedInput {
    bytes: Vec<u8>,
    budget: Arc<AtomicUsize>,
}

impl Drop for QueuedInput {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes.len(), Ordering::AcqRel);
    }
}

#[derive(Clone)]
pub struct InputHandle {
    outgoing: tokio_mpsc::Sender<QueuedInput>,
    budget: Arc<AtomicUsize>,
}

struct MotionSlot {
    latest: Mutex<Option<Vec<u8>>>,
    scheduled: AtomicBool,
    closed: AtomicBool,
    peer: PeerEndpoint,
    commands: tokio_mpsc::UnboundedSender<TransportCommand>,
}

pub struct MotionHandle {
    slot: Arc<MotionSlot>,
}

impl MotionHandle {
    pub fn try_send(&self, frame: &MotionFrame) -> Result<(), String> {
        if self.slot.closed.load(Ordering::Acquire) {
            return Err("motion handle is closed".into());
        }
        let payload = protocol_v2::encode_motion(frame)
            .map_err(|error| format!("invalid motion frame: {error:?}"))?;
        *self
            .slot
            .latest
            .lock()
            .map_err(|_| "motion slot lock poisoned".to_string())? = Some(payload);
        schedule_motion(&self.slot)
    }
}

impl Drop for MotionHandle {
    fn drop(&mut self) {
        self.slot.closed.store(true, Ordering::Release);
        if let Ok(mut latest) = self.slot.latest.lock() {
            *latest = None;
        }
    }
}

fn schedule_motion(slot: &Arc<MotionSlot>) -> Result<(), String> {
    if slot.closed.load(Ordering::Acquire) {
        return Err("motion handle is closed".into());
    }
    if slot.scheduled.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    if slot
        .commands
        .send(TransportCommand::FlushMotion {
            slot: Arc::clone(slot),
        })
        .is_err()
    {
        slot.scheduled.store(false, Ordering::Release);
        return Err("QUIC transport is stopped".into());
    }
    Ok(())
}

impl InputHandle {
    pub fn try_send(&self, frame: &CriticalFrame) -> Result<(), String> {
        let bytes = protocol_v2::encode_critical(frame)
            .map_err(|error| format!("invalid critical input frame: {error:?}"))?;
        reserve_input_bytes(&self.budget, bytes.len())?;
        self.outgoing
            .try_send(QueuedInput {
                bytes,
                budget: Arc::clone(&self.budget),
            })
            .map_err(|error| match error {
                tokio_mpsc::error::TrySendError::Full(_) => {
                    format!("critical input queue is full ({INPUT_QUEUE_FRAMES} frames)")
                }
                tokio_mpsc::error::TrySendError::Closed(_) => {
                    "critical input stream is closed".into()
                }
            })
    }
}

fn reserve_input_bytes(budget: &AtomicUsize, added: usize) -> Result<(), String> {
    let mut current = budget.load(Ordering::Acquire);
    loop {
        let next = current
            .checked_add(added)
            .ok_or_else(|| "critical input byte budget overflow".to_string())?;
        if next > INPUT_QUEUE_BYTES {
            return Err(format!(
                "critical input byte budget exceeded ({INPUT_QUEUE_BYTES} bytes)"
            ));
        }
        match budget.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}

impl ControlHandle {
    pub fn try_send(&self, frame: ControlFrame) -> Result<(), String> {
        self.outgoing.try_send(frame).map_err(|error| match error {
            tokio_mpsc::error::TrySendError::Full(_) => {
                format!("control queue is full ({CONTROL_QUEUE_FRAMES} frames)")
            }
            tokio_mpsc::error::TrySendError::Closed(_) => "control stream is closed".into(),
        })
    }
}

/// Consecutive-failure record for one peer address. Shared between the
/// caller-facing handle (fast-fail check) and the transport loop (updates).
#[derive(Debug, Clone, Copy)]
struct PeerHealth {
    consecutive_failures: u32,
    last_failure: Instant,
}

type HealthMap = Arc<Mutex<HashMap<String, PeerHealth>>>;

fn peer_fast_fail_active(health: &HealthMap, addr: &str) -> bool {
    health
        .lock()
        .map(|health| {
            health.get(addr).is_some_and(|entry| {
                entry.consecutive_failures >= DATAGRAM_FAIL_THRESHOLD
                    && entry.last_failure.elapsed() < DATAGRAM_RETRY_WINDOW
            })
        })
        .unwrap_or(false)
}

fn record_peer_failure(health: &HealthMap, addr: &str, error: &str) {
    let Ok(mut health) = health.lock() else {
        return;
    };
    if health.len() >= MAX_HEALTH_PEERS && !health.contains_key(addr) {
        if let Some(stale) = health
            .iter()
            .min_by_key(|(_, entry)| entry.last_failure)
            .map(|(key, _)| key.clone())
        {
            health.remove(&stale);
        }
    }
    let now = Instant::now();
    let entry = health.entry(addr.to_string()).or_insert(PeerHealth {
        consecutive_failures: 0,
        last_failure: now,
    });
    entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
    entry.last_failure = now;
    // Log the first failure and the transition into fast-fail; everything in
    // between and every muted retry is debug. The old unconditional warn wrote
    // a disk line every few seconds for as long as a peer stayed unreachable.
    match entry.consecutive_failures {
        1 => log::warn!("QUIC send to {addr} failed: {error}"),
        DATAGRAM_FAIL_THRESHOLD => log::warn!(
            "QUIC sends to {addr} keep failing; muting attempts to one probe per {}s: {error}",
            DATAGRAM_RETRY_WINDOW.as_secs()
        ),
        _ => log::debug!("QUIC send to {addr} still failing: {error}"),
    }
}

fn record_peer_success(health: &HealthMap, addr: &str) {
    let Ok(mut health) = health.lock() else {
        return;
    };
    if let Some(entry) = health.remove(addr) {
        if entry.consecutive_failures >= DATAGRAM_FAIL_THRESHOLD {
            log::info!("QUIC sends to {addr} recovered");
        }
    }
}

#[derive(Clone)]
pub struct TransportHandle {
    commands: tokio_mpsc::UnboundedSender<TransportCommand>,
    port: u16,
    public_key: String,
    peer_health: HealthMap,
    trust_store: TrustedPeerRegistry,
    bulk_memory: Arc<BulkMemoryBudget>,
}

impl TransportHandle {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    pub fn peer(&self, addr: String, public_key: String, protocol_version: u16) -> PeerEndpoint {
        PeerEndpoint {
            addr,
            public_key,
            protocol_version,
        }
    }

    pub fn send_datagram(&self, peer: PeerEndpoint, payload: Vec<u8>) -> Result<(), String> {
        if payload.len() > MAX_DATAGRAM_BYTES {
            return Err(format!(
                "QUIC datagram is too large: {} bytes",
                payload.len()
            ));
        }
        // Fail fast while the peer is known-dead so the input layer releases
        // the cursor instead of streaming moves into a black hole.
        if peer_fast_fail_active(&self.peer_health, &peer.addr) {
            return Err(format!(
                "QUIC peer {} unreachable ({DATAGRAM_FAIL_THRESHOLD}+ consecutive failures)",
                peer.addr
            ));
        }

        self.commands
            .send(TransportCommand::SendDatagram { peer, payload })
            .map_err(|_| "QUIC transport is stopped".to_string())
    }

    pub fn send_stream_expect_ack(
        &self,
        peer: PeerEndpoint,
        payload: Vec<u8>,
    ) -> Result<(), String> {
        if payload.len() > MAX_STREAM_BYTES {
            return Err(format!(
                "QUIC stream payload is too large: {} bytes",
                payload.len()
            ));
        }
        if peer_fast_fail_active(&self.peer_health, &peer.addr) {
            return Err(format!(
                "QUIC peer {} unreachable ({DATAGRAM_FAIL_THRESHOLD}+ consecutive failures)",
                peer.addr
            ));
        }
        let reservation = self.bulk_memory.reserve(payload.len())?;

        let (result_tx, result_rx) = mpsc::channel();
        self.commands
            .send(TransportCommand::SendStream {
                peer,
                payload,
                result: result_tx,
                _reservation: reservation,
            })
            .map_err(|_| "QUIC transport is stopped".to_string())?;
        result_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "QUIC stream send timed out".to_string())?
    }

    pub fn shutdown(&self) {
        let _ = self.commands.send(TransportCommand::Shutdown);
    }

    pub fn replace_trusted_peers(&self, peers: Vec<TrustedPeer>) -> Result<(), String> {
        self.trust_store.replace(peers)
    }

    pub fn control_peer(
        &self,
        peer_id: &str,
        role: PeerRole,
        addr: String,
        protocol_version: u16,
    ) -> Result<ControlPeer, String> {
        self.trust_store
            .control_peer(peer_id, role, addr, protocol_version)
    }

    pub fn trusted_bulk_peer(
        &self,
        peer_id: &str,
        role: PeerRole,
        addr: String,
        protocol_version: u16,
    ) -> Result<PeerEndpoint, String> {
        self.trust_store
            .control_peer(peer_id, role, addr, protocol_version)
            .map(|peer| peer.endpoint)
    }

    pub fn open_control(
        &self,
        peer: ControlPeer,
        on_frame: OutboundControlHandler,
    ) -> Result<ControlHandle, String> {
        let (outgoing, receiver) = tokio_mpsc::channel(CONTROL_QUEUE_FRAMES);
        self.commands
            .send(TransportCommand::OpenControl {
                peer,
                outgoing: receiver,
                responses: outgoing.clone(),
                on_frame,
            })
            .map_err(|_| "QUIC transport is stopped".to_string())?;
        Ok(ControlHandle { outgoing })
    }

    pub fn open_input(&self, peer: ControlPeer) -> Result<InputHandle, String> {
        if peer.role != PeerRole::Receiver {
            return Err("critical input stream target must be a trusted receiver".into());
        }
        let (outgoing, receiver) = tokio_mpsc::channel(INPUT_QUEUE_FRAMES);
        let budget = Arc::new(AtomicUsize::new(0));
        self.commands
            .send(TransportCommand::OpenInput {
                peer,
                outgoing: receiver,
            })
            .map_err(|_| "QUIC transport is stopped".to_string())?;
        Ok(InputHandle { outgoing, budget })
    }

    pub fn open_motion(&self, peer: ControlPeer) -> Result<MotionHandle, String> {
        if peer.role != PeerRole::Receiver {
            return Err("motion target must be a trusted receiver".into());
        }
        Ok(MotionHandle {
            slot: Arc::new(MotionSlot {
                latest: Mutex::new(None),
                scheduled: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                peer: peer.endpoint,
                commands: self.commands.clone(),
            }),
        })
    }
}

enum TransportCommand {
    SendDatagram {
        peer: PeerEndpoint,
        payload: Vec<u8>,
    },
    SendStream {
        peer: PeerEndpoint,
        payload: Vec<u8>,
        result: mpsc::Sender<Result<(), String>>,
        _reservation: BulkMemoryReservation,
    },
    OpenControl {
        peer: ControlPeer,
        outgoing: tokio_mpsc::Receiver<ControlFrame>,
        responses: tokio_mpsc::Sender<ControlFrame>,
        on_frame: OutboundControlHandler,
    },
    OpenInput {
        peer: ControlPeer,
        outgoing: tokio_mpsc::Receiver<QueuedInput>,
    },
    FlushMotion {
        slot: Arc<MotionSlot>,
    },
    Shutdown,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct PeerKey {
    addr: SocketAddr,
    public_key: String,
}

pub fn start(
    preferred_port: u16,
    identity_dir: PathBuf,
    trust_store: TrustedPeerRegistry,
    on_datagram: DatagramHandler,
    on_stream: StreamHandler,
    on_control: ControlHandler,
    on_input: InputHandler,
    on_input_closed: InputClosedHandler,
) -> Result<TransportHandle, String> {
    // Load (or create-and-persist) this machine's transport identity *before*
    // spawning the runtime thread so a stable public key is reused across
    // restarts/updates. A churning key breaks the peer's certificate pinning
    // and its paired-controllers authorization until both sides re-pair.
    let identity = load_or_create_identity(&identity_dir)?;
    let (ready_tx, ready_rx) = mpsc::channel();
    let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
    let peer_health: HealthMap = Arc::new(Mutex::new(HashMap::new()));
    let bulk_memory = BulkMemoryBudget::new(MAX_BULK_MEMORY_BYTES);
    let loop_bulk_memory = Arc::clone(&bulk_memory);
    let loop_health = Arc::clone(&peer_health);
    let loop_trust_store = trust_store.clone();

    thread::Builder::new()
        .name("mykvm-quic-transport".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("mykvm-quic")
                .worker_threads(QUIC_WORKER_THREADS)
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("failed to start QUIC runtime: {error}")));
                    return;
                }
            };

            runtime.block_on(run_transport(
                preferred_port,
                identity,
                loop_trust_store,
                command_rx,
                on_datagram,
                on_stream,
                on_control,
                on_input,
                on_input_closed,
                loop_health,
                loop_bulk_memory,
                ready_tx,
            ));
        })
        .map_err(|error| format!("failed to spawn QUIC transport thread: {error}"))?;

    let ready = ready_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "QUIC transport did not become ready".to_string())??;

    Ok(TransportHandle {
        commands: command_tx,
        port: ready.port,
        public_key: ready.public_key,
        peer_health,
        trust_store,
        bulk_memory,
    })
}

struct ReadyTransport {
    port: u16,
    public_key: String,
}

/// A cached peer connection, or a marker that a background task is already
/// establishing one (so a burst of mouse moves cannot spawn a connect storm).
enum ConnectionSlot {
    Connecting,
    Ready(quinn::Connection),
}

type ConnectionMap = Arc<Mutex<HashMap<PeerKey, ConnectionSlot>>>;

async fn run_transport(
    preferred_port: u16,
    identity: TransportIdentity,
    trust_store: TrustedPeerRegistry,
    mut commands: tokio_mpsc::UnboundedReceiver<TransportCommand>,
    on_datagram: DatagramHandler,
    on_stream: StreamHandler,
    on_control: ControlHandler,
    on_input: InputHandler,
    on_input_closed: InputClosedHandler,
    health: HealthMap,
    bulk_memory: Arc<BulkMemoryBudget>,
    ready_tx: mpsc::Sender<Result<ReadyTransport, String>>,
) {
    let (endpoint, public_key) = match bind_endpoint(preferred_port, &identity) {
        Ok(bound) => bound,
        Err(error) => {
            let _ = ready_tx.send(Err(error));
            return;
        }
    };

    let port = match endpoint.local_addr() {
        Ok(addr) => addr.port(),
        Err(error) => {
            let _ = ready_tx.send(Err(format!("failed to read QUIC port: {error}")));
            return;
        }
    };

    let _ = ready_tx.send(Ok(ReadyTransport { port, public_key }));
    spawn_accept_loop(
        endpoint.clone(),
        trust_store,
        on_datagram,
        on_stream,
        on_control,
        on_input,
        on_input_closed,
        Arc::clone(&bulk_memory),
    );

    // The command loop must never await network progress: one dead peer's 2s
    // connect timeout or one 48MB stream write would stall every queued input
    // datagram behind it (the "periodic input freeze + warn every 4s" bug).
    // Datagrams go out synchronously on established connections; connection
    // establishment and stream sends run in spawned tasks.
    let connections: ConnectionMap = Arc::new(Mutex::new(HashMap::new()));
    let stream_slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_STREAMS));
    let control_slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONTROL_CONNECTIONS));
    let input_slots = Arc::new(tokio::sync::Semaphore::new(MAX_INPUT_CONNECTIONS));
    while let Some(command) = commands.recv().await {
        match command {
            TransportCommand::SendDatagram { peer, payload } => {
                send_datagram_nonblocking(
                    &endpoint,
                    &identity,
                    &connections,
                    &health,
                    peer,
                    payload,
                );
            }
            TransportCommand::SendStream {
                peer,
                payload,
                result,
                _reservation,
            } => {
                let Ok(permit) = Arc::clone(&stream_slots).try_acquire_owned() else {
                    let _ = result.send(Err(format!(
                        "QUIC stream queue is full ({MAX_CONCURRENT_STREAMS} in flight)"
                    )));
                    continue;
                };
                let endpoint = endpoint.clone();
                let connections = Arc::clone(&connections);
                let health = Arc::clone(&health);
                let identity = identity.clone();
                tokio::spawn(async move {
                    let _reservation = _reservation;
                    let outcome = send_stream_task(
                        &endpoint,
                        &identity,
                        &connections,
                        &health,
                        peer,
                        payload,
                    )
                    .await;
                    if let Err(error) = &outcome {
                        log::warn!("QUIC stream send failed: {error}");
                    }
                    let _ = result.send(outcome);
                    drop(permit);
                });
            }
            TransportCommand::OpenControl {
                peer,
                outgoing,
                responses,
                on_frame,
            } => {
                let Ok(permit) = Arc::clone(&control_slots).try_acquire_owned() else {
                    continue;
                };
                let endpoint = endpoint.clone();
                let identity = identity.clone();
                let connections = Arc::clone(&connections);
                let health = Arc::clone(&health);
                tokio::spawn(async move {
                    if let Err(error) = run_outbound_control(
                        &endpoint,
                        &identity,
                        &connections,
                        &health,
                        peer,
                        outgoing,
                        responses,
                        on_frame,
                    )
                    .await
                    {
                        log::warn!("QUIC control stream stopped: {error}");
                    }
                    drop(permit);
                });
            }
            TransportCommand::OpenInput { peer, outgoing } => {
                let Ok(permit) = Arc::clone(&input_slots).try_acquire_owned() else {
                    continue;
                };
                let endpoint = endpoint.clone();
                let identity = identity.clone();
                let connections = Arc::clone(&connections);
                let health = Arc::clone(&health);
                tokio::spawn(async move {
                    if let Err(error) = run_outbound_input(
                        &endpoint,
                        &identity,
                        &connections,
                        &health,
                        peer,
                        outgoing,
                    )
                    .await
                    {
                        log::warn!("QUIC critical input stream stopped: {error}");
                    }
                    drop(permit);
                });
            }
            TransportCommand::FlushMotion { slot } => {
                flush_motion_nonblocking(&endpoint, &identity, &connections, &health, slot);
            }
            TransportCommand::Shutdown => break,
        }
    }

    endpoint.close(0_u32.into(), b"shutdown");
    endpoint.wait_idle().await;
}

fn bind_endpoint(
    preferred_port: u16,
    identity: &TransportIdentity,
) -> Result<(Endpoint, String), String> {
    let runtime = quinn::default_runtime()
        .ok_or_else(|| "no async runtime available for QUIC endpoint".to_string())?;
    let mut last_error = None;

    for port in candidate_ports(preferred_port) {
        let server_config = server_config(identity)?;
        let socket = match bind_reusable_quic_socket(port) {
            Ok(socket) => socket,
            Err(error) => {
                last_error = Some(error.to_string());
                continue;
            }
        };
        // Build the endpoint from our own reuse-enabled socket instead of
        // Endpoint::server (which binds a plain socket without SO_REUSEADDR).
        match Endpoint::new(
            quinn::EndpointConfig::default(),
            Some(server_config),
            socket,
            runtime.clone(),
        ) {
            Ok(endpoint) => return Ok((endpoint, identity.public_key.clone())),
            Err(error) => last_error = Some(error.to_string()),
        }
    }

    Err(format!(
        "failed to bind QUIC port: {}",
        last_error.unwrap_or_else(|| "no candidate ports available".into())
    ))
}

/// Bind the QUIC endpoint's UDP socket with address reuse enabled, mirroring the
/// discovery socket. Without `SO_REUSEADDR` a fresh endpoint cannot re-grab the
/// same QUIC port while the previous process's socket is still tearing down — on
/// an admin-restart, app relaunch, or runtime restart the port silently drifts
/// upward (47834 -> 47835 ...) and the controller keeps targeting the stale port
/// until re-discovery propagates the new one, which is the intermittent "shows
/// online but the cursor won't cross" symptom.
fn bind_reusable_quic_socket(port: u16) -> std::io::Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    socket.bind(&address.into())?;
    Ok(socket.into())
}

/// This machine's persisted QUIC transport identity. The advertised
/// `public_key` is the base64 of the certificate DER — peers pin it during
/// discovery, so it MUST stay stable across restarts.
#[derive(Clone)]
struct TransportIdentity {
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
    public_key: String,
}

const QUIC_CERT_FILE: &str = "quic-transport-cert.der";
const QUIC_KEY_FILE: &str = "quic-transport-key.der";

/// Load the persisted self-signed cert/key, or generate one and persist it on
/// first run (or when the stored files are missing/corrupt). Without this the
/// identity was regenerated on every launch, rotating the advertised public
/// key and breaking the peer's pinned-cert handshake / pairing authorization.
fn load_or_create_identity(dir: &Path) -> Result<TransportIdentity, String> {
    let cert_path = dir.join(QUIC_CERT_FILE);
    let key_path = dir.join(QUIC_KEY_FILE);

    if let (Ok(cert_der), Ok(key_der)) = (fs::read(&cert_path), fs::read(&key_path)) {
        if !cert_der.is_empty() && !key_der.is_empty() {
            return Ok(TransportIdentity {
                public_key: BASE64.encode(&cert_der),
                cert_der,
                key_der,
            });
        }
    }

    let generated =
        rcgen::generate_simple_self_signed(vec![SERVER_NAME.into(), "localhost".into()])
            .map_err(|error| format!("failed to generate QUIC certificate: {error}"))?;
    let cert_der = generated.cert.der().to_vec();
    let key_der = generated.key_pair.serialize_der();

    if let Err(error) = fs::create_dir_all(dir) {
        log::warn!(
            "failed to create QUIC identity dir {}: {error}",
            dir.display()
        );
    }
    if let Err(error) = fs::write(&cert_path, &cert_der) {
        log::warn!("failed to persist QUIC certificate: {error}");
    }
    if let Err(error) = fs::write(&key_path, &key_der) {
        log::warn!("failed to persist QUIC key: {error}");
    }

    Ok(TransportIdentity {
        public_key: BASE64.encode(&cert_der),
        cert_der,
        key_der,
    })
}

fn candidate_ports(preferred_port: u16) -> Vec<u16> {
    let start = preferred_port.max(1024);
    let mut ports = Vec::new();
    for offset in 0..PORT_SCAN_COUNT {
        let Some(port) = start.checked_add(offset) else {
            break;
        };
        if port == 0 {
            continue;
        }
        ports.push(port);
    }
    ports.push(0);
    ports
}

#[derive(Debug)]
struct PresentedClientCertVerifier {
    supported: WebPkiSupportedAlgorithms,
}

impl PresentedClientCertVerifier {
    fn new() -> Self {
        Self {
            supported: default_provider().signature_verification_algorithms,
        }
    }
}

impl ClientCertVerifier for PresentedClientCertVerifier {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        if end_entity.is_empty() {
            return Err(rustls::Error::General("empty client certificate".into()));
        }
        // This verifies presentation only. The TLS CertificateVerify signature
        // is checked below; application authorization is an exact match in the
        // persisted TrustStore after the connection is established.
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported.supported_schemes()
    }

    fn client_auth_mandatory(&self) -> bool {
        false
    }
}

fn server_config(identity: &TransportIdentity) -> Result<ServerConfig, String> {
    let cert_der = CertificateDer::from(identity.cert_der.clone());
    let key_der = PrivatePkcs8KeyDer::from(identity.key_der.clone());
    let mut crypto = rustls::ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| format!("failed to build QUIC server crypto: {error}"))?
        .with_client_cert_verifier(Arc::new(PresentedClientCertVerifier::new()))
        .with_single_cert(vec![cert_der], key_der.into())
        .map_err(|error| format!("failed to build QUIC server config: {error}"))?;
    crypto.alpn_protocols = vec![ALPN_V2.to_vec()];
    let quic_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)
        .map_err(|error| format!("failed to build QUIC server config: {error}"))?;
    let mut config = ServerConfig::with_crypto(Arc::new(quic_crypto));
    config.transport = Arc::new(tuned_transport_config());

    Ok(config)
}

/// Shared QUIC transport tuning. The keep-alive interval holds connections open
/// through idle periods so the first input event after the machine has been
/// sitting unused does not pay a fresh handshake (the "laggy after idle" feel),
/// while the idle timeout still reaps connections to peers that truly vanished.
fn tuned_transport_config() -> quinn::TransportConfig {
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(64_u32.into());
    // Keep-alive well under the idle timeout so a healthy link never drops, but
    // keep the idle timeout short: when a client vanishes (e.g. it is killed and
    // reinstalled during an app upgrade) the controller's cached connection must
    // close on its own within a few seconds. Otherwise the controller keeps
    // reusing the now-dead connection after the client comes back, so input
    // silently goes nowhere until the user toggles the runtime to force a
    // reconnect. 10 s tolerates brief LAN/Wi-Fi hiccups while auto-recovering
    // across the typical upgrade downtime without any manual toggle.
    transport.keep_alive_interval(Some(Duration::from_secs(3)));
    if let Ok(timeout) = quinn::IdleTimeout::try_from(Duration::from_secs(10)) {
        transport.max_idle_timeout(Some(timeout));
    }
    transport
}

/// Certificate-pinning verifier for the QUIC transport.
///
/// Each peer generates a fresh self-signed certificate at startup and
/// advertises it during discovery. We pin *exactly* that certificate instead
/// of running a WebPKI chain/CA validation over a self-signed leaf — the latter
/// is brittle across platforms and was rejecting otherwise valid peers with
/// `invalid peer certificate: BadSignature` (Mac↔Windows handshakes failed, so
/// input/clipboard never connected). The handshake signature is still verified
/// against the pinned certificate's key via the ring provider, so a peer must
/// prove it actually holds the advertised key — pinning by bytes alone is not
/// enough on its own.
#[derive(Debug)]
struct PinnedCertVerifier {
    pinned: CertificateDer<'static>,
    supported: WebPkiSupportedAlgorithms,
}

impl PinnedCertVerifier {
    fn new(pinned: CertificateDer<'static>) -> Self {
        Self {
            pinned,
            supported: default_provider().signature_verification_algorithms,
        }
    }
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.pinned.as_ref() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "peer certificate does not match the pinned transport certificate".to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported.supported_schemes()
    }
}

fn client_config(
    peer: &PeerEndpoint,
    identity: &TransportIdentity,
) -> Result<ClientConfig, String> {
    if peer.protocol_version != PROTOCOL_VERSION {
        return Err(format!(
            "unsupported peer transport protocol version {}",
            peer.protocol_version
        ));
    }

    let cert_der = BASE64
        .decode(peer.public_key.as_bytes())
        .map_err(|error| format!("invalid peer transport public key: {error}"))?;
    let pinned = CertificateDer::from(cert_der);

    // QUIC is TLS 1.3 only; pin the advertised certificate with our own verifier
    // rather than WebPKI root validation.
    let mut crypto = rustls::ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| format!("failed to build QUIC client crypto: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedCertVerifier::new(pinned)))
        .with_client_auth_cert(
            vec![CertificateDer::from(identity.cert_der.clone())],
            PrivatePkcs8KeyDer::from(identity.key_der.clone()).into(),
        )
        .map_err(|error| format!("failed to configure QUIC client identity: {error}"))?;
    crypto.alpn_protocols = vec![ALPN_V2.to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
        .map_err(|error| format!("failed to build QUIC client config: {error}"))?;
    let mut config = ClientConfig::new(Arc::new(quic_crypto));
    config.transport_config(Arc::new(tuned_transport_config()));
    Ok(config)
}

fn spawn_accept_loop(
    endpoint: Endpoint,
    trust_store: TrustedPeerRegistry,
    on_datagram: DatagramHandler,
    on_stream: StreamHandler,
    on_control: ControlHandler,
    on_input: InputHandler,
    on_input_closed: InputClosedHandler,
    bulk_memory: Arc<BulkMemoryBudget>,
) {
    let generations = Arc::new(AtomicU64::new(1));
    let inbound_stream_slots = Arc::new(tokio::sync::Semaphore::new(MAX_INBOUND_STREAMS));
    tokio::spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let remote = incoming.remote_address();
            let on_datagram = Arc::clone(&on_datagram);
            let on_stream = Arc::clone(&on_stream);
            let on_control = Arc::clone(&on_control);
            let on_input = Arc::clone(&on_input);
            let on_input_closed = Arc::clone(&on_input_closed);
            let trust_store = trust_store.clone();
            let inbound_stream_slots = Arc::clone(&inbound_stream_slots);
            let bulk_memory = Arc::clone(&bulk_memory);
            let generation = generations.fetch_add(1, Ordering::Relaxed);

            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        let peer = connection_peer(&connection, &trust_store, remote, generation);
                        if let ConnectionPeer::Authenticated(authenticated) = &peer {
                            spawn_datagram_reader(
                                connection.clone(),
                                authenticated.clone(),
                                on_datagram,
                            );
                        }
                        spawn_stream_reader(
                            connection,
                            peer,
                            on_stream,
                            on_control,
                            on_input,
                            on_input_closed,
                            inbound_stream_slots,
                            bulk_memory,
                        );
                    }
                    Err(error) => {
                        log::warn!("QUIC incoming connection failed from {remote}: {error}");
                    }
                }
            });
        }
    });
}

fn spawn_datagram_reader(
    connection: quinn::Connection,
    peer: AuthenticatedPeer,
    on_datagram: DatagramHandler,
) {
    tokio::spawn(async move {
        loop {
            match connection.read_datagram().await {
                Ok(payload) => on_datagram(payload.to_vec(), peer.clone()),
                Err(error) => {
                    log::debug!(
                        "QUIC datagram reader stopped for {}: {error}",
                        peer.remote_addr
                    );
                    break;
                }
            }
        }
    });
}

fn connection_peer(
    connection: &quinn::Connection,
    trust_store: &TrustedPeerRegistry,
    remote_addr: SocketAddr,
    connection_generation: u64,
) -> ConnectionPeer {
    let certificate = connection
        .peer_identity()
        .and_then(|identity| identity.downcast::<Vec<CertificateDer<'static>>>().ok())
        .and_then(|chain| chain.first().map(|cert| cert.as_ref().to_vec()));
    let authenticated = certificate
        .as_deref()
        .and_then(|cert| trust_store.authenticate(cert, remote_addr, connection_generation))
        .map(ConnectionPeer::Authenticated);
    authenticated.unwrap_or(ConnectionPeer::Unauthenticated {
        remote_addr,
        connection_generation,
        presented_certificate: certificate.map(|cert| BASE64.encode(cert)),
    })
}

fn spawn_stream_reader(
    connection: quinn::Connection,
    peer: ConnectionPeer,
    on_stream: StreamHandler,
    on_control: ControlHandler,
    on_input: InputHandler,
    on_input_closed: InputClosedHandler,
    inbound_stream_slots: Arc<tokio::sync::Semaphore>,
    bulk_memory: Arc<BulkMemoryBudget>,
) {
    let control_active = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let input_active = Arc::new(std::sync::atomic::AtomicBool::new(false));
    tokio::spawn(async move {
        loop {
            match connection.accept_bi().await {
                Ok((mut send, mut recv)) => {
                    let Ok(stream_permit) = Arc::clone(&inbound_stream_slots).try_acquire_owned()
                    else {
                        let _ = send.finish();
                        let _ = recv.stop(0_u32.into());
                        continue;
                    };
                    let on_stream = Arc::clone(&on_stream);
                    let on_control = Arc::clone(&on_control);
                    let on_input = Arc::clone(&on_input);
                    let on_input_closed = Arc::clone(&on_input_closed);
                    let peer = peer.clone();
                    let connection = connection.clone();
                    let control_active = Arc::clone(&control_active);
                    let input_active = Arc::clone(&input_active);
                    let bulk_memory = Arc::clone(&bulk_memory);
                    tokio::spawn(async move {
                        let _stream_permit = stream_permit;
                        let mut preface = [0_u8; 4];
                        if let Err(error) = recv.read_exact(&mut preface).await {
                            log::warn!("QUIC stream preface read failed: {error}");
                            return;
                        }
                        if &preface == CONTROL_PREFACE {
                            let ConnectionPeer::Authenticated(authenticated) = peer else {
                                let _ = send.finish();
                                return;
                            };
                            if control_active.swap(true, Ordering::AcqRel) {
                                let _ = send.finish();
                                return;
                            }
                            let outcome = run_inbound_control_stream(
                                &mut send,
                                &mut recv,
                                authenticated,
                                on_control,
                            )
                            .await;
                            control_active.store(false, Ordering::Release);
                            if let Err(error) = outcome {
                                log::warn!("QUIC inbound control stream stopped: {error}");
                            }
                            let _ = send.finish();
                            return;
                        }
                        if &preface == INPUT_PREFACE {
                            let ConnectionPeer::Authenticated(authenticated) = peer else {
                                let _ = send.finish();
                                return;
                            };
                            if authenticated.role != PeerRole::Controller
                                || input_active.swap(true, Ordering::AcqRel)
                            {
                                let _ = send.finish();
                                return;
                            }
                            let outcome = run_inbound_input_stream(
                                &mut recv,
                                authenticated.clone(),
                                on_input,
                            )
                            .await;
                            input_active.store(false, Ordering::Release);
                            let reason = outcome
                                .as_ref()
                                .err()
                                .cloned()
                                .unwrap_or_else(|| "critical input stream closed".into());
                            on_input_closed(authenticated, reason);
                            if let Err(error) = outcome {
                                log::warn!("QUIC inbound critical input stopped: {error}");
                            }
                            let _ = send.finish();
                            return;
                        }

                        // Generic stream handlers may decode a second owned
                        // representation and, for a clipboard image, a 32 MiB
                        // RGBA buffer. Reserve its full worst-case peak before
                        // reading so only one large bulk payload is decoded at a
                        // time while a concurrent configured-size text send can
                        // still finish. Control/input streams bypass this branch.
                        let Ok(_bulk_reservation) =
                            bulk_memory.reserve(INBOUND_BULK_RESERVATION_BYTES)
                        else {
                            let _ = recv.stop(0_u32.into());
                            let _ = send.finish();
                            return;
                        };
                        match recv.read_to_end(MAX_STREAM_BYTES - preface.len()).await {
                            Ok(payload) => {
                                let mut complete =
                                    Vec::with_capacity(preface.len() + payload.len());
                                complete.extend_from_slice(&preface);
                                complete.extend_from_slice(&payload);
                                drop(payload);
                                let was_unauthenticated =
                                    matches!(peer, ConnectionPeer::Unauthenticated { .. });
                                let accepted =
                                    tokio::task::spawn_blocking(move || on_stream(complete, peer))
                                        .await
                                        .unwrap_or(false);
                                let ack: &[u8] = if accepted { b"ok" } else { b"reject" };
                                let _ = send.write_all(ack).await;
                                let _ = send.finish();
                                if accepted && was_unauthenticated {
                                    // A successful unauthenticated stream can only be the
                                    // bounded manual pairing exchange. Force a new TLS
                                    // connection so the newly persisted certificate is
                                    // evaluated and bound before any data channel is opened.
                                    connection.close(0_u32.into(), b"pairing-complete");
                                }
                            }
                            Err(error) => {
                                log::warn!("QUIC stream read failed: {error}");
                            }
                        }
                    });
                }
                Err(error) => {
                    log::debug!("QUIC stream reader stopped: {error}");
                    break;
                }
            }
        }
    });
}

async fn run_inbound_input_stream(
    recv: &mut quinn::RecvStream,
    peer: AuthenticatedPeer,
    on_input: InputHandler,
) -> Result<(), String> {
    let mut decoder = CriticalDecoder::default();
    loop {
        let chunk = recv
            .read_chunk(4096, true)
            .await
            .map_err(|error| format!("critical input read failed: {error}"))?;
        let Some(chunk) = chunk else {
            return decoder.finish().map_err(|error| {
                format!("critical input stream ended with partial frame: {error:?}")
            });
        };
        for frame in decoder
            .push(&chunk.bytes)
            .map_err(|error| format!("invalid critical input frame: {error:?}"))?
        {
            if !on_input(frame, peer.clone()) {
                return Err("critical input handler rejected frame".into());
            }
        }
    }
}

async fn run_inbound_control_stream(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    peer: AuthenticatedPeer,
    on_control: ControlHandler,
) -> Result<(), String> {
    let mut decoder = ControlDecoder::default();
    let mut window_started = Instant::now();
    let mut frames_in_window = 0_u32;
    loop {
        let chunk = recv
            .read_chunk(4096, true)
            .await
            .map_err(|error| format!("control read failed: {error}"))?;
        let Some(chunk) = chunk else {
            return decoder
                .finish()
                .map_err(|error| format!("control stream ended with partial frame: {error:?}"));
        };
        let frames = decoder
            .push(&chunk.bytes)
            .map_err(|error| format!("invalid control frame: {error:?}"))?;
        enforce_control_rate(
            &mut window_started,
            &mut frames_in_window,
            frames.len() as u32,
        )?;
        for frame in frames {
            if let Some(response) = on_control(frame, peer.clone()) {
                let encoded = protocol_v2::encode_control(&response)
                    .map_err(|error| format!("invalid control response: {error:?}"))?;
                send.write_all(&encoded)
                    .await
                    .map_err(|error| format!("control response write failed: {error}"))?;
            }
        }
    }
}

fn enforce_control_rate(
    window_started: &mut Instant,
    frames_in_window: &mut u32,
    added: u32,
) -> Result<(), String> {
    if window_started.elapsed() >= Duration::from_secs(1) {
        *window_started = Instant::now();
        *frames_in_window = 0;
    }
    *frames_in_window = frames_in_window.saturating_add(added);
    if *frames_in_window > MAX_CONTROL_FRAMES_PER_SECOND {
        Err(format!(
            "control frame rate exceeded ({MAX_CONTROL_FRAMES_PER_SECOND}/s)"
        ))
    } else {
        Ok(())
    }
}

/// Datagram send that never awaits: an established connection queues the
/// payload synchronously (quinn's `send_datagram` is not async); a missing or
/// dead connection drops this payload and kicks off a background connect —
/// input datagrams are latest-wins, the next move follows within ~8ms, and
/// `warm_quic_peer` keeps connections pre-established outside crossings.
fn flush_motion_nonblocking(
    endpoint: &Endpoint,
    identity: &TransportIdentity,
    connections: &ConnectionMap,
    health: &HealthMap,
    slot: Arc<MotionSlot>,
) {
    if slot.closed.load(Ordering::Acquire) {
        slot.scheduled.store(false, Ordering::Release);
        return;
    }
    let payload = slot.latest.lock().ok().and_then(|mut latest| latest.take());
    if let Some(payload) = payload.filter(|_| !slot.closed.load(Ordering::Acquire)) {
        send_datagram_nonblocking(
            endpoint,
            identity,
            connections,
            health,
            slot.peer.clone(),
            payload,
        );
    }

    slot.scheduled.store(false, Ordering::Release);
    let has_newer = slot
        .latest
        .lock()
        .map(|latest| latest.is_some())
        .unwrap_or(false);
    if has_newer && !slot.closed.load(Ordering::Acquire) {
        let _ = schedule_motion(&slot);
    }
}

fn send_datagram_nonblocking(
    endpoint: &Endpoint,
    identity: &TransportIdentity,
    connections: &ConnectionMap,
    health: &HealthMap,
    peer: PeerEndpoint,
    payload: Vec<u8>,
) {
    let key = match peer_key(&peer) {
        Ok(key) => key,
        Err(error) => {
            record_peer_failure(health, &peer.addr, &error);
            return;
        }
    };

    let ready = {
        let Ok(mut map) = connections.lock() else {
            return;
        };
        match map.get(&key) {
            Some(ConnectionSlot::Ready(connection)) if connection.close_reason().is_none() => {
                Some(connection.clone())
            }
            // A background task is already dialing this peer; drop the payload.
            Some(ConnectionSlot::Connecting) => return,
            _ => {
                map.remove(&key);
                None
            }
        }
    };

    if let Some(connection) = ready {
        match connection.send_datagram(payload.into()) {
            Ok(()) => record_peer_success(health, &peer.addr),
            Err(error) => {
                if let Ok(mut map) = connections.lock() {
                    map.remove(&key);
                }
                record_peer_failure(health, &peer.addr, &error.to_string());
            }
        }
        return;
    }

    // Known-dead peer inside its retry window: skip even the background dial
    // so an unreachable box costs nothing between probes.
    if peer_fast_fail_active(health, &peer.addr) {
        return;
    }
    if let Ok(mut map) = connections.lock() {
        map.insert(key.clone(), ConnectionSlot::Connecting);
    }
    let endpoint = endpoint.clone();
    let connections = Arc::clone(connections);
    let health = Arc::clone(health);
    let identity = identity.clone();
    tokio::spawn(async move {
        match establish_connection(&endpoint, &identity, &peer, &key).await {
            Ok(connection) => {
                if let Ok(mut map) = connections.lock() {
                    map.insert(key, ConnectionSlot::Ready(connection));
                }
                record_peer_success(&health, &peer.addr);
            }
            Err(error) => {
                if let Ok(mut map) = connections.lock() {
                    map.remove(&key);
                }
                record_peer_failure(&health, &peer.addr, &error);
            }
        }
    });
}

/// Stream send running inside its own task: reuses a ready connection or
/// dials one inline (a racing datagram dial at worst produces one redundant
/// connection that is dropped on replacement — streams are rare).
async fn send_stream_task(
    endpoint: &Endpoint,
    identity: &TransportIdentity,
    connections: &ConnectionMap,
    health: &HealthMap,
    peer: PeerEndpoint,
    payload: Vec<u8>,
) -> Result<(), String> {
    let key = peer_key(&peer)?;
    let existing = {
        let Ok(map) = connections.lock() else {
            return Err("QUIC connection map is poisoned".into());
        };
        match map.get(&key) {
            Some(ConnectionSlot::Ready(connection)) if connection.close_reason().is_none() => {
                Some(connection.clone())
            }
            _ => None,
        }
    };
    let connection = match existing {
        Some(connection) => connection,
        None => match establish_connection(endpoint, identity, &peer, &key).await {
            Ok(connection) => {
                if let Ok(mut map) = connections.lock() {
                    map.insert(key.clone(), ConnectionSlot::Ready(connection.clone()));
                }
                record_peer_success(health, &peer.addr);
                connection
            }
            Err(error) => {
                record_peer_failure(health, &peer.addr, &error);
                return Err(error);
            }
        },
    };

    let result = send_stream_on_connection(connection, payload).await;
    if result.is_err() {
        if let Ok(mut map) = connections.lock() {
            map.remove(&key);
        }
    }
    result
}

async fn run_outbound_control(
    endpoint: &Endpoint,
    identity: &TransportIdentity,
    connections: &ConnectionMap,
    health: &HealthMap,
    peer: ControlPeer,
    mut outgoing: tokio_mpsc::Receiver<ControlFrame>,
    responses: tokio_mpsc::Sender<ControlFrame>,
    on_frame: OutboundControlHandler,
) -> Result<(), String> {
    let key = peer_key(&peer.endpoint)?;
    let existing = connections.lock().ok().and_then(|map| match map.get(&key) {
        Some(ConnectionSlot::Ready(connection)) if connection.close_reason().is_none() => {
            Some(connection.clone())
        }
        _ => None,
    });
    let connection = match existing {
        Some(connection) => connection,
        None => {
            let connection = establish_connection(endpoint, identity, &peer.endpoint, &key).await?;
            if let Ok(mut map) = connections.lock() {
                map.insert(key.clone(), ConnectionSlot::Ready(connection.clone()));
            }
            record_peer_success(health, &peer.endpoint.addr);
            connection
        }
    };
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| format!("failed to open control stream: {error}"))?;
    send.write_all(CONTROL_PREFACE)
        .await
        .map_err(|error| format!("failed to write control preface: {error}"))?;

    let writer = tokio::spawn(async move {
        while let Some(frame) = outgoing.recv().await {
            let encoded = protocol_v2::encode_control(&frame)
                .map_err(|error| format!("invalid outgoing control frame: {error:?}"))?;
            send.write_all(&encoded)
                .await
                .map_err(|error| format!("control write failed: {error}"))?;
        }
        send.finish()
            .map_err(|error| format!("control finish failed: {error}"))
    });

    let read_result = read_outbound_control(&mut recv, responses, on_frame).await;
    writer.abort();
    if let Ok(mut map) = connections.lock() {
        map.remove(&key);
    }
    match read_result {
        Ok(()) => Ok(()),
        Err(error) => {
            connection.close(0_u32.into(), b"invalid-control");
            Err(format!(
                "control peer {} role {:?} revision {}: {error}",
                peer.peer_id, peer.role, peer.trust_revision
            ))
        }
    }
}

async fn run_outbound_input(
    endpoint: &Endpoint,
    identity: &TransportIdentity,
    connections: &ConnectionMap,
    health: &HealthMap,
    peer: ControlPeer,
    mut outgoing: tokio_mpsc::Receiver<QueuedInput>,
) -> Result<(), String> {
    let key = peer_key(&peer.endpoint)?;
    let existing = connections.lock().ok().and_then(|map| match map.get(&key) {
        Some(ConnectionSlot::Ready(connection)) if connection.close_reason().is_none() => {
            Some(connection.clone())
        }
        _ => None,
    });
    let connection = match existing {
        Some(connection) => connection,
        None => {
            let connection = establish_connection(endpoint, identity, &peer.endpoint, &key).await?;
            if let Ok(mut map) = connections.lock() {
                map.insert(key.clone(), ConnectionSlot::Ready(connection.clone()));
            }
            record_peer_success(health, &peer.endpoint.addr);
            connection
        }
    };
    let (mut send, _recv) = connection
        .open_bi()
        .await
        .map_err(|error| format!("failed to open critical input stream: {error}"))?;
    send.write_all(INPUT_PREFACE)
        .await
        .map_err(|error| format!("failed to write critical input preface: {error}"))?;
    while let Some(queued) = outgoing.recv().await {
        send.write_all(&queued.bytes)
            .await
            .map_err(|error| format!("critical input write failed: {error}"))?;
    }
    send.finish()
        .map_err(|error| format!("critical input finish failed: {error}"))?;
    Ok(())
}

async fn read_outbound_control(
    recv: &mut quinn::RecvStream,
    responses: tokio_mpsc::Sender<ControlFrame>,
    on_frame: OutboundControlHandler,
) -> Result<(), String> {
    let mut decoder = ControlDecoder::default();
    let mut window_started = Instant::now();
    let mut frames_in_window = 0_u32;
    loop {
        let chunk = recv
            .read_chunk(4096, true)
            .await
            .map_err(|error| format!("control read failed: {error}"))?;
        let Some(chunk) = chunk else {
            return decoder
                .finish()
                .map_err(|error| format!("control stream ended with partial frame: {error:?}"));
        };
        let frames = decoder
            .push(&chunk.bytes)
            .map_err(|error| format!("invalid control frame: {error:?}"))?;
        enforce_control_rate(
            &mut window_started,
            &mut frames_in_window,
            frames.len() as u32,
        )?;
        for frame in frames {
            if let Some(response) = on_frame(frame) {
                responses
                    .try_send(response)
                    .map_err(|_| "control response queue is full or closed".to_string())?;
            }
        }
    }
}

async fn send_stream_on_connection(
    connection: quinn::Connection,
    payload: Vec<u8>,
) -> Result<(), String> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| format!("failed to open QUIC stream: {error}"))?;
    send.write_all(&payload)
        .await
        .map_err(|error| format!("failed to write QUIC stream: {error}"))?;
    send.finish()
        .map_err(|error| format!("failed to finish QUIC stream: {error}"))?;
    match tokio::time::timeout(Duration::from_millis(500), recv.read_to_end(64)).await {
        Ok(Ok(bytes)) => verify_stream_ack(&bytes),
        Ok(Err(error)) => Err(format!("failed to read QUIC stream ack: {error}")),
        Err(_) => Err("QUIC stream ack timed out".into()),
    }
}

fn verify_stream_ack(bytes: &[u8]) -> Result<(), String> {
    if bytes == b"ok" {
        Ok(())
    } else {
        Err(format!(
            "QUIC stream receiver rejected payload: {}",
            String::from_utf8_lossy(bytes)
        ))
    }
}

async fn establish_connection(
    endpoint: &Endpoint,
    identity: &TransportIdentity,
    peer: &PeerEndpoint,
    key: &PeerKey,
) -> Result<quinn::Connection, String> {
    let config = client_config(peer, identity)?;
    let connecting = endpoint
        .connect_with(config, key.addr, SERVER_NAME)
        .map_err(|error| format!("failed to start QUIC connection to {}: {error}", key.addr))?;
    tokio::time::timeout(Duration::from_secs(2), connecting)
        .await
        .map_err(|_| format!("QUIC connection to {} timed out", key.addr))?
        .map_err(|error| format!("failed to connect QUIC to {}: {error}", key.addr))
}

fn peer_key(peer: &PeerEndpoint) -> Result<PeerKey, String> {
    Ok(PeerKey {
        addr: resolve_peer_addr(&peer.addr)?,
        public_key: peer.public_key.clone(),
    })
}

fn resolve_peer_addr(addr: &str) -> Result<SocketAddr, String> {
    addr.to_socket_addrs()
        .map_err(|error| format!("invalid peer QUIC address {addr}: {error}"))?
        .next()
        .ok_or_else(|| format!("peer QUIC address {addr} did not resolve"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a27_bulk_memory_budget_counts_bytes_and_releases_reservations() {
        let budget = BulkMemoryBudget::new(128);
        let first = budget.reserve(80).expect("first reservation");
        assert!(budget.reserve(49).is_err());
        let second = budget.reserve(48).expect("remaining budget");
        assert_eq!(budget.used.load(Ordering::Acquire), 128);
        assert!(budget.reserve(1).is_err());
        drop(first);
        assert_eq!(budget.used.load(Ordering::Acquire), 48);
        let third = budget.reserve(80).expect("released bytes are reusable");
        drop((second, third));
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
        assert!(budget.reserve(usize::MAX).is_err());
    }

    #[test]
    fn peer_health_fast_fails_after_threshold_and_recovers_on_success() {
        let health: HealthMap = Arc::new(Mutex::new(HashMap::new()));
        let addr = "10.0.0.9:47834";

        for strikes in 1..DATAGRAM_FAIL_THRESHOLD {
            record_peer_failure(&health, addr, "timeout");
            assert!(
                !peer_fast_fail_active(&health, addr),
                "{strikes} failures must not fast-fail yet"
            );
        }
        record_peer_failure(&health, addr, "timeout");
        assert!(
            peer_fast_fail_active(&health, addr),
            "reaching the threshold enters fast-fail"
        );
        assert!(
            !peer_fast_fail_active(&health, "10.0.0.8:47834"),
            "health is tracked per peer address"
        );

        record_peer_success(&health, addr);
        assert!(
            !peer_fast_fail_active(&health, addr),
            "one successful send clears the fast-fail state"
        );
    }

    #[test]
    fn inbound_stream_budget_rejects_excess_work_and_recovers() {
        let slots = Arc::new(tokio::sync::Semaphore::new(MAX_INBOUND_STREAMS));
        let mut permits = Vec::new();
        for _ in 0..MAX_INBOUND_STREAMS {
            permits.push(Arc::clone(&slots).try_acquire_owned().unwrap());
        }
        assert!(Arc::clone(&slots).try_acquire_owned().is_err());
        permits.pop();
        assert!(Arc::clone(&slots).try_acquire_owned().is_ok());
    }

    fn make_cert() -> CertificateDer<'static> {
        rcgen::generate_simple_self_signed(vec!["mykvm.local".to_string()])
            .unwrap()
            .cert
            .der()
            .clone()
    }

    fn make_identity() -> TransportIdentity {
        let generated = rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_string()]).unwrap();
        let cert_der = generated.cert.der().to_vec();
        TransportIdentity {
            public_key: BASE64.encode(&cert_der),
            cert_der,
            key_der: generated.key_pair.serialize_der(),
        }
    }

    #[test]
    fn pinned_verifier_accepts_matching_cert_and_rejects_others() {
        let pinned = make_cert();
        let other = make_cert();
        let verifier = PinnedCertVerifier::new(pinned.clone());
        let name = ServerName::try_from("mykvm.local").unwrap();
        let now = UnixTime::now();

        assert!(
            verifier
                .verify_server_cert(&pinned, &[], &name, &[], now)
                .is_ok(),
            "the advertised certificate must be accepted"
        );
        assert!(
            verifier
                .verify_server_cert(&other, &[], &name, &[], now)
                .is_err(),
            "a different certificate must be rejected"
        );
    }

    #[test]
    fn client_config_builds_from_advertised_public_key() {
        let peer = PeerEndpoint {
            addr: "127.0.0.1:47834".to_string(),
            public_key: BASE64.encode(make_cert().as_ref()),
            protocol_version: PROTOCOL_VERSION,
        };
        assert!(client_config(&peer, &make_identity()).is_ok());
    }

    #[test]
    fn client_config_rejects_protocol_version_mismatch() {
        let peer = PeerEndpoint {
            addr: "127.0.0.1:47834".to_string(),
            public_key: BASE64.encode(make_cert().as_ref()),
            protocol_version: PROTOCOL_VERSION + 1,
        };
        assert!(client_config(&peer, &make_identity()).is_err());
    }

    #[test]
    fn trust_store_authenticates_exact_certificate_and_binds_context() {
        let identity = make_identity();
        let store = TrustedPeerRegistry::default();
        store
            .replace(vec![TrustedPeer {
                peer_id: "controller-a".into(),
                certificate: identity.public_key.clone(),
                role: PeerRole::Controller,
                trust_revision: 7,
            }])
            .unwrap();
        let address = "10.0.0.2:47834".parse().unwrap();
        let authenticated = store
            .authenticate(&identity.cert_der, address, 19)
            .expect("trusted certificate");
        assert_eq!(authenticated.peer_id, "controller-a");
        assert_eq!(authenticated.role, PeerRole::Controller);
        assert_eq!(authenticated.trust_revision, 7);
        assert_eq!(authenticated.connection_generation, 19);
        assert_eq!(authenticated.remote_addr, address);
    }

    #[test]
    fn trust_store_rejects_unknown_or_rotated_certificate() {
        let trusted = make_identity();
        let rotated = make_identity();
        let store = TrustedPeerRegistry::default();
        store
            .replace(vec![TrustedPeer {
                peer_id: "controller-a".into(),
                certificate: trusted.public_key,
                role: PeerRole::Controller,
                trust_revision: 1,
            }])
            .unwrap();
        assert!(store
            .authenticate(&rotated.cert_der, "10.0.0.2:47834".parse().unwrap(), 1)
            .is_none());
    }

    #[test]
    fn trust_store_rejects_duplicate_identity_or_certificate() {
        let identity = make_identity();
        let store = TrustedPeerRegistry::default();
        let duplicate = vec![
            TrustedPeer {
                peer_id: "peer-a".into(),
                certificate: identity.public_key.clone(),
                role: PeerRole::Controller,
                trust_revision: 1,
            },
            TrustedPeer {
                peer_id: "peer-b".into(),
                certificate: identity.public_key,
                role: PeerRole::Receiver,
                trust_revision: 2,
            },
        ];
        assert!(store.replace(duplicate).is_err());
    }

    #[test]
    fn stream_ack_rejects_non_ok_payloads() {
        assert!(verify_stream_ack(b"ok").is_ok());
        assert!(verify_stream_ack(b"reject").is_err());
    }

    #[test]
    fn peer_key_uses_resolved_addr_and_public_key() {
        let key = peer_key(&PeerEndpoint {
            addr: "127.0.0.1:47834".into(),
            public_key: "pinned-cert".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .expect("peer key");

        assert_eq!(key.addr, "127.0.0.1:47834".parse::<SocketAddr>().unwrap());
        assert_eq!(key.public_key, "pinned-cert");
    }

    #[test]
    fn quic_runtime_uses_small_worker_pool() {
        assert_eq!(QUIC_WORKER_THREADS, 2);
    }

    #[test]
    fn identity_is_stable_across_reloads() {
        let dir = std::env::temp_dir().join("mykvm-quic-identity-stability-test");
        let _ = fs::remove_dir_all(&dir);

        let first = load_or_create_identity(&dir).expect("first identity load");
        let second = load_or_create_identity(&dir).expect("second identity load");

        assert_eq!(
            first.public_key, second.public_key,
            "the advertised public key must survive a reload"
        );
        assert_eq!(first.cert_der, second.cert_der);
        assert_eq!(first.key_der, second.key_der);
        assert!(!first.public_key.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn loopback_transport_binds_presented_certificate_to_connection_context() {
        let suffix = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(format!("mykvm-auth-loopback-{suffix}"));
        let controller_dir = root.join("controller");
        let receiver_dir = root.join("receiver");
        let controller_identity = load_or_create_identity(&controller_dir).unwrap();
        let receiver_identity = load_or_create_identity(&receiver_dir).unwrap();
        let (seen_tx, seen_rx) = mpsc::channel();
        let receiver_port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let controller_port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();

        let receiver = start(
            receiver_port,
            receiver_dir,
            TrustedPeerRegistry::new(vec![TrustedPeer {
                peer_id: "controller-a".into(),
                certificate: controller_identity.public_key.clone(),
                role: PeerRole::Controller,
                trust_revision: 4,
            }])
            .unwrap(),
            Arc::new(|_, _| {}),
            Arc::new(move |payload, peer| {
                let _ = seen_tx.send((payload, peer));
                true
            }),
            Arc::new(|_, _| None),
            Arc::new(|_, _| false),
            Arc::new(|_, _| {}),
        )
        .unwrap();
        let controller = start(
            controller_port,
            controller_dir,
            TrustedPeerRegistry::new(vec![TrustedPeer {
                peer_id: "receiver-a".into(),
                certificate: receiver_identity.public_key.clone(),
                role: PeerRole::Receiver,
                trust_revision: 2,
            }])
            .unwrap(),
            Arc::new(|_, _| {}),
            Arc::new(|_, _| false),
            Arc::new(|_, _| None),
            Arc::new(|_, _| false),
            Arc::new(|_, _| {}),
        )
        .unwrap();

        assert!(controller
            .trusted_bulk_peer(
                "receiver-a",
                PeerRole::Controller,
                format!("127.0.0.1:{}", receiver.port()),
                PROTOCOL_VERSION,
            )
            .is_err());
        let peer = controller
            .trusted_bulk_peer(
                "receiver-a",
                PeerRole::Receiver,
                format!("127.0.0.1:{}", receiver.port()),
                PROTOCOL_VERSION,
            )
            .unwrap();
        controller
            .send_stream_expect_ack(peer, b"authenticated".to_vec())
            .unwrap();
        let (payload, peer) = seen_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(payload, b"authenticated");
        let ConnectionPeer::Authenticated(peer) = peer else {
            panic!("trusted client certificate was not bound to the connection")
        };
        assert_eq!(peer.peer_id, "controller-a");
        assert_eq!(peer.role, PeerRole::Controller);
        assert_eq!(peer.trust_revision, 4);
        assert!(peer.connection_generation > 0);

        controller.shutdown();
        receiver.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn untrusted_loopback_connection_never_reaches_datagram_handler() {
        let suffix = format!("{}-untrusted", std::process::id());
        let root = std::env::temp_dir().join(format!("mykvm-auth-loopback-{suffix}"));
        let controller_dir = root.join("controller");
        let receiver_dir = root.join("receiver");
        let receiver_identity = load_or_create_identity(&receiver_dir).unwrap();
        let port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (datagram_tx, datagram_rx) = mpsc::channel();
        let receiver = start(
            port,
            receiver_dir,
            TrustedPeerRegistry::default(),
            Arc::new(move |_, _| {
                let _ = datagram_tx.send(());
            }),
            Arc::new(|_, _| false),
            Arc::new(|_, _| None),
            Arc::new(|_, _| false),
            Arc::new(|_, _| {}),
        )
        .unwrap();
        let controller = start(
            port.saturating_add(64),
            controller_dir,
            TrustedPeerRegistry::default(),
            Arc::new(|_, _| {}),
            Arc::new(|_, _| false),
            Arc::new(|_, _| None),
            Arc::new(|_, _| false),
            Arc::new(|_, _| {}),
        )
        .unwrap();
        let peer = controller.peer(
            format!("127.0.0.1:{}", receiver.port()),
            receiver_identity.public_key,
            PROTOCOL_VERSION,
        );
        for _ in 0..8 {
            controller
                .send_datagram(peer.clone(), b"input".to_vec())
                .unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(datagram_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());
        controller.shutdown();
        receiver.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn authenticated_control_stream_exchanges_multiple_frames_without_eof() {
        let suffix = format!("{}-control", std::process::id());
        let root = std::env::temp_dir().join(format!("mykvm-control-loopback-{suffix}"));
        let controller_dir = root.join("controller");
        let receiver_dir = root.join("receiver");
        let controller_identity = load_or_create_identity(&controller_dir).unwrap();
        let receiver_identity = load_or_create_identity(&receiver_dir).unwrap();
        let port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let handshake = Arc::new(Mutex::new(
            protocol_v2::ReceiverHandshake::new(
                "controller-a".into(),
                protocol_v2::BootId([2; 16]),
            )
            .unwrap(),
        ));
        let receiver_handshake = Arc::clone(&handshake);
        let receiver = start(
            port,
            receiver_dir,
            TrustedPeerRegistry::new(vec![TrustedPeer {
                peer_id: "controller-a".into(),
                certificate: controller_identity.public_key,
                role: PeerRole::Controller,
                trust_revision: 1,
            }])
            .unwrap(),
            Arc::new(|_, _| {}),
            Arc::new(|_, _| false),
            Arc::new(move |frame, peer| {
                if peer.role != PeerRole::Controller || peer.peer_id != "controller-a" {
                    return None;
                }
                receiver_handshake
                    .lock()
                    .ok()?
                    .handle(&frame)
                    .ok()
                    .flatten()
            }),
            Arc::new(|_, _| false),
            Arc::new(|_, _| {}),
        )
        .unwrap();
        let controller = start(
            port.saturating_add(64),
            controller_dir,
            TrustedPeerRegistry::new(vec![TrustedPeer {
                peer_id: "receiver-a".into(),
                certificate: receiver_identity.public_key,
                role: PeerRole::Receiver,
                trust_revision: 1,
            }])
            .unwrap(),
            Arc::new(|_, _| {}),
            Arc::new(|_, _| false),
            Arc::new(|_, _| None),
            Arc::new(|_, _| false),
            Arc::new(|_, _| {}),
        )
        .unwrap();
        let peer = controller
            .control_peer(
                "receiver-a",
                PeerRole::Receiver,
                format!("127.0.0.1:{}", receiver.port()),
                PROTOCOL_VERSION,
            )
            .unwrap();
        let (frame_tx, frame_rx) = mpsc::channel();
        let control = controller
            .open_control(
                peer,
                Arc::new(move |frame| {
                    let _ = frame_tx.send(frame);
                    None
                }),
            )
            .unwrap();
        control
            .try_send(ControlFrame::Hello {
                boot_id: protocol_v2::BootId([1; 16]),
                peer_id: "controller-a".into(),
                role: protocol_v2::DeviceRole::Controller,
                capabilities: vec!["control_v2".into(), "input_v2".into()],
            })
            .unwrap();
        control
            .try_send(ControlFrame::Prepare {
                request_id: 7,
                target_display: "mac-main".into(),
                layout_revision: 1,
            })
            .unwrap();
        let ready = frame_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            ready,
            ControlFrame::Ready {
                request_id: 7,
                input_ready: true,
                ..
            }
        ));
        let session = protocol_v2::SessionId {
            controller_boot: protocol_v2::BootId([1; 16]),
            receiver_boot: protocol_v2::BootId([2; 16]),
            nonce: [3; 16],
        };
        control
            .try_send(ControlFrame::Commit {
                request_id: 7,
                session_id: session,
            })
            .unwrap();
        assert_eq!(
            frame_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            ControlFrame::CommitAck {
                session_id: session
            }
        );
        drop(control);
        controller.shutdown();
        receiver.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn control_handle_is_bounded_and_rate_limit_fails_closed() {
        let (sender, _receiver) = tokio_mpsc::channel(CONTROL_QUEUE_FRAMES);
        let handle = ControlHandle { outgoing: sender };
        let frame = ControlFrame::Reject {
            code: "test".into(),
            detail: "bounded".into(),
        };
        for _ in 0..CONTROL_QUEUE_FRAMES {
            handle.try_send(frame.clone()).unwrap();
        }
        assert!(handle.try_send(frame).is_err());

        let mut started = Instant::now();
        let mut count = 0;
        assert!(
            enforce_control_rate(&mut started, &mut count, MAX_CONTROL_FRAMES_PER_SECOND).is_ok()
        );
        assert!(enforce_control_rate(&mut started, &mut count, 1).is_err());
    }

    #[test]
    fn authenticated_input_stream_preserves_reliable_frame_order() {
        let suffix = format!("{}-input", std::process::id());
        let root = std::env::temp_dir().join(format!("mykvm-input-loopback-{suffix}"));
        let controller_dir = root.join("controller");
        let receiver_dir = root.join("receiver");
        let controller_identity = load_or_create_identity(&controller_dir).unwrap();
        let receiver_identity = load_or_create_identity(&receiver_dir).unwrap();
        let port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (input_tx, input_rx) = mpsc::channel();
        let (closed_tx, closed_rx) = mpsc::channel();
        let receiver = start(
            port,
            receiver_dir,
            TrustedPeerRegistry::new(vec![TrustedPeer {
                peer_id: "controller-a".into(),
                certificate: controller_identity.public_key,
                role: PeerRole::Controller,
                trust_revision: 1,
            }])
            .unwrap(),
            Arc::new(|_, _| {}),
            Arc::new(|_, _| false),
            Arc::new(|_, _| None),
            Arc::new(move |frame, peer| input_tx.send((frame, peer)).is_ok()),
            Arc::new(move |peer, reason| {
                let _ = closed_tx.send((peer, reason));
            }),
        )
        .unwrap();
        let controller = start(
            port.saturating_add(64),
            controller_dir,
            TrustedPeerRegistry::new(vec![TrustedPeer {
                peer_id: "receiver-a".into(),
                certificate: receiver_identity.public_key,
                role: PeerRole::Receiver,
                trust_revision: 1,
            }])
            .unwrap(),
            Arc::new(|_, _| {}),
            Arc::new(|_, _| false),
            Arc::new(|_, _| None),
            Arc::new(|_, _| false),
            Arc::new(|_, _| {}),
        )
        .unwrap();
        let peer = controller
            .control_peer(
                "receiver-a",
                PeerRole::Receiver,
                format!("127.0.0.1:{}", receiver.port()),
                PROTOCOL_VERSION,
            )
            .unwrap();
        let input = controller.open_input(peer).unwrap();
        let session = protocol_v2::SessionId {
            controller_boot: protocol_v2::BootId([1; 16]),
            receiver_boot: protocol_v2::BootId([2; 16]),
            nonce: [3; 16],
        };
        for (sequence, down) in [(1, true), (2, false)] {
            input
                .try_send(&CriticalFrame {
                    session_id: session,
                    sequence,
                    event: protocol_v2::CriticalEvent::Key {
                        key_code: 65,
                        scan_code: 30,
                        extended: false,
                        down,
                    },
                })
                .unwrap();
        }
        for expected in [1, 2] {
            let (frame, peer) = input_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(frame.sequence, expected);
            assert_eq!(peer.peer_id, "controller-a");
            assert_eq!(peer.role, PeerRole::Controller);
        }
        drop(input);
        let (closed_peer, reason) = closed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(closed_peer.peer_id, "controller-a");
        assert!(reason.contains("closed"));
        controller.shutdown();
        receiver.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn blocked_bulk_handlers_do_not_starve_critical_input() {
        let suffix = format!("{}-bulk-fairness", std::process::id());
        let root = std::env::temp_dir().join(format!("mykvm-input-loopback-{suffix}"));
        let controller_dir = root.join("controller");
        let receiver_dir = root.join("receiver");
        let controller_identity = load_or_create_identity(&controller_dir).unwrap();
        let receiver_identity = load_or_create_identity(&receiver_dir).unwrap();
        let port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (bulk_started_tx, bulk_started_rx) = mpsc::channel();
        let (bulk_release_tx, bulk_release_rx) = mpsc::channel();
        let bulk_release_rx = Arc::new(Mutex::new(bulk_release_rx));
        let (input_tx, input_rx) = mpsc::channel();
        let receiver = start(
            port,
            receiver_dir,
            TrustedPeerRegistry::new(vec![TrustedPeer {
                peer_id: "controller-a".into(),
                certificate: controller_identity.public_key,
                role: PeerRole::Controller,
                trust_revision: 1,
            }])
            .unwrap(),
            Arc::new(|_, _| {}),
            Arc::new(move |_, _| {
                let _ = bulk_started_tx.send(());
                bulk_release_rx
                    .lock()
                    .ok()
                    .and_then(|receiver| receiver.recv_timeout(Duration::from_secs(2)).ok())
                    .is_some()
            }),
            Arc::new(|_, _| None),
            Arc::new(move |frame, _| input_tx.send(frame).is_ok()),
            Arc::new(|_, _| {}),
        )
        .unwrap();
        let controller = start(
            port.saturating_add(64),
            controller_dir,
            TrustedPeerRegistry::new(vec![TrustedPeer {
                peer_id: "receiver-a".into(),
                certificate: receiver_identity.public_key.clone(),
                role: PeerRole::Receiver,
                trust_revision: 1,
            }])
            .unwrap(),
            Arc::new(|_, _| {}),
            Arc::new(|_, _| false),
            Arc::new(|_, _| None),
            Arc::new(|_, _| false),
            Arc::new(|_, _| {}),
        )
        .unwrap();
        let endpoint = controller.peer(
            format!("127.0.0.1:{}", receiver.port()),
            receiver_identity.public_key,
            PROTOCOL_VERSION,
        );
        let mut bulk_threads = Vec::new();
        for marker in [1_u8, 2] {
            let controller = controller.clone();
            let endpoint = endpoint.clone();
            bulk_threads.push(thread::spawn(move || {
                controller.send_stream_expect_ack(endpoint, vec![marker; 1024])
            }));
        }
        bulk_started_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();

        let peer = controller
            .control_peer(
                "receiver-a",
                PeerRole::Receiver,
                format!("127.0.0.1:{}", receiver.port()),
                PROTOCOL_VERSION,
            )
            .unwrap();
        let input = controller.open_input(peer).unwrap();
        input
            .try_send(&CriticalFrame {
                session_id: protocol_v2::SessionId {
                    controller_boot: protocol_v2::BootId([1; 16]),
                    receiver_boot: protocol_v2::BootId([2; 16]),
                    nonce: [3; 16],
                },
                sequence: 1,
                event: protocol_v2::CriticalEvent::Key {
                    key_code: 65,
                    scan_code: 30,
                    extended: false,
                    down: true,
                },
            })
            .unwrap();
        assert_eq!(
            input_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .sequence,
            1
        );

        bulk_release_tx.send(()).unwrap();
        let mut accepted = 0;
        let mut rejected = 0;
        for thread in bulk_threads {
            match thread.join().unwrap() {
                Ok(()) => accepted += 1,
                Err(_) => rejected += 1,
            }
        }
        assert_eq!((accepted, rejected), (1, 1));
        drop(input);
        controller.shutdown();
        receiver.shutdown();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn critical_input_queue_enforces_frame_and_byte_budgets() {
        let (outgoing, receiver) = tokio_mpsc::channel(INPUT_QUEUE_FRAMES);
        let budget = Arc::new(AtomicUsize::new(0));
        let handle = InputHandle {
            outgoing,
            budget: Arc::clone(&budget),
        };
        let frame = CriticalFrame {
            session_id: protocol_v2::SessionId {
                controller_boot: protocol_v2::BootId([1; 16]),
                receiver_boot: protocol_v2::BootId([2; 16]),
                nonce: [3; 16],
            },
            sequence: 1,
            event: protocol_v2::CriticalEvent::Key {
                key_code: 65,
                scan_code: 30,
                extended: false,
                down: true,
            },
        };
        for _ in 0..INPUT_QUEUE_FRAMES {
            handle.try_send(&frame).unwrap();
        }
        assert!(handle.try_send(&frame).is_err());
        drop(receiver);
        assert_eq!(budget.load(Ordering::Acquire), 0);
        assert!(reserve_input_bytes(&budget, INPUT_QUEUE_BYTES).is_ok());
        assert!(reserve_input_bytes(&budget, 1).is_err());
    }

    #[test]
    fn motion_slot_keeps_only_the_latest_frame_and_one_flush_command() {
        let (commands, mut receiver) = tokio_mpsc::unbounded_channel();
        let slot = Arc::new(MotionSlot {
            latest: Mutex::new(None),
            scheduled: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            peer: PeerEndpoint {
                addr: "127.0.0.1:47834".into(),
                public_key: "pinned-cert".into(),
                protocol_version: PROTOCOL_VERSION,
            },
            commands,
        });
        let handle = MotionHandle {
            slot: Arc::clone(&slot),
        };
        let session_id = protocol_v2::SessionId {
            controller_boot: protocol_v2::BootId([1; 16]),
            receiver_boot: protocol_v2::BootId([2; 16]),
            nonce: [3; 16],
        };

        for sequence in 1..=100 {
            handle
                .try_send(&MotionFrame {
                    session_id,
                    display_id: "mac-main".into(),
                    layout_revision: 1,
                    sequence,
                    required_reliable_sequence: 7,
                    x: sequence as i32,
                    y: -(sequence as i32),
                })
                .unwrap();
        }

        let queued_slot = match receiver.try_recv().unwrap() {
            TransportCommand::FlushMotion { slot } => slot,
            _ => panic!("motion slot must queue a flush command"),
        };
        assert!(receiver.try_recv().is_err(), "only one flush may be queued");
        assert!(Arc::ptr_eq(&slot, &queued_slot));
        let latest = slot.latest.lock().unwrap().clone().unwrap();
        assert_eq!(
            protocol_v2::decode_motion(&latest).unwrap(),
            MotionFrame {
                session_id,
                display_id: "mac-main".into(),
                layout_revision: 1,
                sequence: 100,
                required_reliable_sequence: 7,
                x: 100,
                y: -100,
            }
        );

        drop(handle);
        assert!(slot.closed.load(Ordering::Acquire));
        assert!(slot.latest.lock().unwrap().is_none());
        assert!(schedule_motion(&slot).is_err());
    }
}
