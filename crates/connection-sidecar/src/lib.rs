use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAX_EVENT_BYTES: usize = 16 * 1024;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EVENTS: u64 = 100_000;
const MAX_BATCH_ID: u32 = 100_000;
const MAX_RECONNECT_ATTEMPTS: u32 = 1_000_000;
const MAX_RECONNECT_DELAY_MS: u64 = 60_000;
const MAX_TIMESTAMP_MS: u64 = i64::MAX as u64;
const MAX_SUBSCRIPTIONS: usize = 10_000;
const MAX_SUBSCRIPTION_BYTES: usize = 256;
const MAX_SESSION_SUBSCRIPTION_BYTES: usize = 1024 * 1024;
const MAX_SESSION_CONNECTIONS: usize = 1_024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Public,
    Market,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionIdentity {
    pub connection_id: String,
    pub batch_id: u32,
    pub route: Route,
    /// Exact stream names passed to the WebSocket subscription URL.
    pub subscriptions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DisconnectReason {
    RemoteClose,
    TransportError,
    HeartbeatTimeout,
    Shutdown,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConnectFailureReason {
    Timeout,
    Dns,
    Transport,
    HandshakeRejected,
    Shutdown,
    Unknown,
}

impl ConnectFailureReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "TIMEOUT",
            Self::Dns => "DNS",
            Self::Transport => "TRANSPORT",
            Self::HandshakeRejected => "HANDSHAKE_REJECTED",
            Self::Shutdown => "SHUTDOWN",
            Self::Unknown => "UNKNOWN",
        }
    }
}

impl DisconnectReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::RemoteClose => "REMOTE_CLOSE",
            Self::TransportError => "TRANSPORT_ERROR",
            Self::HeartbeatTimeout => "HEARTBEAT_TIMEOUT",
            Self::Shutdown => "SHUTDOWN",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Event<'a> {
    schema_version: u8,
    event_id: String,
    source_commit: &'a str,
    session_id: &'a str,
    connection_id: &'a str,
    batch_id: u32,
    route: Route,
    event_type: &'static str,
    event_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    heartbeat_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delay_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subscription_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subscription_set_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSubscription {
    pub connection_id: String,
    pub batch_id: u32,
    pub route: Route,
    pub subscriptions: Vec<String>,
    pub subscription_set_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureManifest {
    pub schema_version: u8,
    pub source_commit: String,
    pub session_id: String,
    pub sidecar_file: String,
    pub sidecar_bytes: u64,
    pub sidecar_sha256: String,
    pub capture_finalized: bool,
    pub capture_complete_through_ms: u64,
    pub finalized_at_ms: u64,
    pub event_count: u64,
    pub connection_subscriptions: Vec<ConnectionSubscription>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Lifecycle {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone)]
struct ConnectionState {
    identity: ConnectionIdentity,
    lifecycle: Lifecycle,
    heartbeat_ids: HashSet<String>,
    pending_pings: HashMap<String, u64>,
    heartbeat_timeout_observed: bool,
    reconnect_scheduled: bool,
}

pub struct SidecarWriter {
    file: File,
    path: PathBuf,
    source_commit: String,
    session_id: String,
    next_event_sequence: u64,
    last_event_at_ms: Option<u64>,
    event_count: u64,
    bytes_written: u64,
    session_subscription_bytes: usize,
    connections: HashMap<String, ConnectionState>,
    poisoned: bool,
}

impl SidecarWriter {
    /// Create a new per-process JSONL sidecar. Existing files are never truncated.
    pub fn create(
        path: impl AsRef<Path>,
        source_commit: &str,
        session_id: &str,
    ) -> io::Result<Self> {
        if !is_lower_hex(source_commit, 40) {
            return Err(invalid(
                "source_commit must be a 40-character lowercase Git SHA",
            ));
        }
        if session_id.is_empty() || session_id.len() > 128 {
            return Err(invalid("session_id must contain 1..=128 bytes"));
        }

        let path = path.as_ref().to_path_buf();
        let file = create_sidecar_file(&path, sync_parent_directory)?;

        Ok(Self {
            file,
            path,
            source_commit: source_commit.to_string(),
            session_id: session_id.to_string(),
            next_event_sequence: 0,
            last_event_at_ms: None,
            event_count: 0,
            bytes_written: 0,
            session_subscription_bytes: 0,
            connections: HashMap::new(),
            poisoned: false,
        })
    }

    pub fn manifest_path(path: impl AsRef<Path>) -> PathBuf {
        path.as_ref().with_extension("manifest.json")
    }

    /// Durably record the attempt before opening the TCP/WebSocket handshake.
    pub fn connect_attempted(
        &mut self,
        identity: &ConnectionIdentity,
        event_at_ms: u64,
    ) -> io::Result<()> {
        self.ensure_usable()?;
        validate_identity(identity)?;
        if self.connections.contains_key(&identity.connection_id) {
            return Err(invalid("connection_id may be opened only once per session"));
        }
        if self.connections.len() >= MAX_SESSION_CONNECTIONS {
            return Err(invalid("session connection count limit exceeded"));
        }
        let subscription_bytes = identity
            .subscriptions
            .iter()
            .fold(0_usize, |total, item| total.saturating_add(item.len()));
        let aggregate_subscription_bytes = self
            .session_subscription_bytes
            .checked_add(subscription_bytes)
            .ok_or_else(|| invalid("session subscription byte count overflow"))?;
        if aggregate_subscription_bytes > MAX_SESSION_SUBSCRIPTION_BYTES {
            return Err(invalid(
                "session subscription metadata exceeds the 1 MiB bound",
            ));
        }
        self.append_event(
            identity,
            "CONNECT_ATTEMPTED",
            event_at_ms,
            None,
            None,
            None,
            None,
            Some(identity.subscriptions.len()),
            Some(subscription_set_sha256(&identity.subscriptions)),
        )?;
        self.session_subscription_bytes = aggregate_subscription_bytes;
        self.connections.insert(
            identity.connection_id.clone(),
            ConnectionState {
                identity: identity.clone(),
                lifecycle: Lifecycle::Connecting,
                heartbeat_ids: HashSet::new(),
                pending_pings: HashMap::new(),
                heartbeat_timeout_observed: false,
                reconnect_scheduled: false,
            },
        );
        Ok(())
    }

    /// Record a DNS, transport, timeout, or handshake failure before CONNECTED.
    pub fn connect_failed(
        &mut self,
        identity: &ConnectionIdentity,
        reason: ConnectFailureReason,
        event_at_ms: u64,
    ) -> io::Result<()> {
        self.ensure_connecting(identity)?;
        self.append_event(
            identity,
            "CONNECT_FAILED",
            event_at_ms,
            None,
            Some(reason.as_str()),
            None,
            None,
            None,
            None,
        )?;
        self.connections
            .get_mut(&identity.connection_id)
            .expect("checked above")
            .lifecycle = Lifecycle::Disconnected;
        Ok(())
    }

    /// A successful handshake can only follow a durable CONNECT_ATTEMPTED event.
    pub fn connected(&mut self, identity: &ConnectionIdentity, event_at_ms: u64) -> io::Result<()> {
        self.ensure_connecting(identity)?;
        self.append_event(
            identity,
            "CONNECTED",
            event_at_ms,
            None,
            None,
            None,
            None,
            Some(identity.subscriptions.len()),
            Some(subscription_set_sha256(&identity.subscriptions)),
        )?;
        self.connections
            .get_mut(&identity.connection_id)
            .expect("checked above")
            .lifecycle = Lifecycle::Connected;
        Ok(())
    }

    pub fn ping_sent(
        &mut self,
        identity: &ConnectionIdentity,
        heartbeat_id: &str,
        event_at_ms: u64,
    ) -> io::Result<()> {
        let state = self.ensure_connected(identity)?;
        if state.heartbeat_timeout_observed {
            return Err(invalid(
                "cannot send a Ping after a heartbeat timeout was observed",
            ));
        }
        if heartbeat_id.is_empty() || heartbeat_id.len() > 128 {
            return Err(invalid("heartbeat_id must contain 1..=128 bytes"));
        }
        {
            let state = self
                .connections
                .get(&identity.connection_id)
                .expect("checked above");
            if state.heartbeat_ids.contains(heartbeat_id) {
                return Err(invalid("heartbeat_id must be unique within a connection"));
            }
        }

        self.append_event(
            identity,
            "PING_SENT",
            event_at_ms,
            Some(heartbeat_id),
            None,
            None,
            None,
            None,
            None,
        )?;
        let state = self
            .connections
            .get_mut(&identity.connection_id)
            .expect("checked above");
        state.heartbeat_ids.insert(heartbeat_id.to_string());
        state
            .pending_pings
            .insert(heartbeat_id.to_string(), event_at_ms);
        Ok(())
    }

    pub fn pong_received(
        &mut self,
        identity: &ConnectionIdentity,
        heartbeat_id: &str,
        event_at_ms: u64,
    ) -> io::Result<()> {
        self.ensure_connected(identity)?;
        let sent_at = self
            .connections
            .get(&identity.connection_id)
            .and_then(|state| state.pending_pings.get(heartbeat_id).copied())
            .ok_or_else(|| invalid("Pong has no matching outstanding Ping"))?;
        if event_at_ms < sent_at || event_at_ms - sent_at > MAX_RECONNECT_DELAY_MS {
            return Err(invalid(
                "Pong arrived outside the 60-second heartbeat bound",
            ));
        }

        self.append_event(
            identity,
            "PONG_RECEIVED",
            event_at_ms,
            Some(heartbeat_id),
            None,
            None,
            None,
            None,
            None,
        )?;
        self.connections
            .get_mut(&identity.connection_id)
            .expect("checked above")
            .pending_pings
            .remove(heartbeat_id);
        Ok(())
    }

    /// Durably resolve one outstanding Ping after the 60-second timeout threshold.
    pub fn heartbeat_timed_out(
        &mut self,
        identity: &ConnectionIdentity,
        heartbeat_id: &str,
        event_at_ms: u64,
    ) -> io::Result<()> {
        self.ensure_connected(identity)?;
        let sent_at = self
            .connections
            .get(&identity.connection_id)
            .and_then(|state| state.pending_pings.get(heartbeat_id).copied())
            .ok_or_else(|| invalid("heartbeat timeout has no matching outstanding Ping"))?;
        if event_at_ms < sent_at || event_at_ms - sent_at <= MAX_RECONNECT_DELAY_MS {
            return Err(invalid(
                "heartbeat timeout requires more than 60 seconds elapsed",
            ));
        }

        self.append_event(
            identity,
            "PING_TIMEOUT",
            event_at_ms,
            Some(heartbeat_id),
            Some("HEARTBEAT_TIMEOUT"),
            None,
            None,
            None,
            None,
        )?;
        let state = self
            .connections
            .get_mut(&identity.connection_id)
            .expect("checked above");
        state.pending_pings.remove(heartbeat_id);
        state.heartbeat_timeout_observed = true;
        Ok(())
    }

    pub fn disconnected(
        &mut self,
        identity: &ConnectionIdentity,
        reason: DisconnectReason,
        event_at_ms: u64,
    ) -> io::Result<()> {
        let state = self.ensure_connected(identity)?;
        if !state.pending_pings.is_empty() {
            return Err(invalid(
                "cannot disconnect while heartbeat Pings remain unanswered",
            ));
        }
        if reason == DisconnectReason::HeartbeatTimeout && !state.heartbeat_timeout_observed {
            return Err(invalid(
                "heartbeat-timeout disconnect requires a recorded PING_TIMEOUT event",
            ));
        }
        if state.heartbeat_timeout_observed && reason != DisconnectReason::HeartbeatTimeout {
            return Err(invalid(
                "a connection with a recorded PING_TIMEOUT must disconnect for heartbeat timeout",
            ));
        }
        self.append_event(
            identity,
            "DISCONNECTED",
            event_at_ms,
            None,
            Some(reason.as_str()),
            None,
            None,
            None,
            None,
        )?;
        self.connections
            .get_mut(&identity.connection_id)
            .expect("checked above")
            .lifecycle = Lifecycle::Disconnected;
        Ok(())
    }

    pub fn reconnect_scheduled(
        &mut self,
        identity: &ConnectionIdentity,
        attempt: u32,
        delay_ms: u64,
        event_at_ms: u64,
    ) -> io::Result<()> {
        let state = self.ensure_disconnected(identity)?;
        if state.reconnect_scheduled {
            return Err(invalid(
                "only one reconnect schedule is allowed per closed socket",
            ));
        }
        if attempt == 0
            || attempt > MAX_RECONNECT_ATTEMPTS
            || delay_ms == 0
            || delay_ms > MAX_RECONNECT_DELAY_MS
        {
            return Err(invalid(
                "reconnect attempt/delay is outside the supported bounds",
            ));
        }

        self.append_event(
            identity,
            "RECONNECT_SCHEDULED",
            event_at_ms,
            None,
            None,
            Some(attempt),
            Some(delay_ms),
            None,
            None,
        )?;
        self.connections
            .get_mut(&identity.connection_id)
            .expect("checked above")
            .reconnect_scheduled = true;
        Ok(())
    }

    /// Seal this process-session sidecar and publish its manifest without replacing
    /// an existing manifest. All sockets must first have a durable DISCONNECTED event.
    pub fn finalize(
        &mut self,
        capture_complete_through_ms: u64,
        finalized_at_ms: u64,
    ) -> io::Result<CaptureManifest> {
        self.ensure_usable()?;
        if self.event_count == 0 {
            return Err(invalid("cannot finalize an empty sidecar"));
        }
        if self.connections.values().any(|state| {
            state.lifecycle != Lifecycle::Disconnected || !state.pending_pings.is_empty()
        }) {
            return Err(invalid(
                "all connection attempts must be resolved before session finalization",
            ));
        }
        if capture_complete_through_ms > MAX_TIMESTAMP_MS || finalized_at_ms > MAX_TIMESTAMP_MS {
            return Err(invalid(
                "capture timestamps exceed the signed 64-bit contract",
            ));
        }
        if self
            .last_event_at_ms
            .is_some_and(|last| capture_complete_through_ms < last)
        {
            return Err(invalid(
                "capture completeness watermark precedes the final event",
            ));
        }
        if finalized_at_ms < capture_complete_through_ms {
            return Err(invalid(
                "finalized_at_ms precedes capture completeness watermark",
            ));
        }

        // Once finalization passes validation it is terminal. Poison before the
        // first filesystem operation so flush, sync, readback, metadata, and
        // publication failures can never leave the writer available for appends.
        self.poisoned = true;
        self.file.flush()?;
        self.file.sync_all()?;

        // Read one bounded snapshot from a single file handle. The manifest must
        // describe exactly the bytes accounted for by this writer, not an
        // out-of-band append, truncation, malformed line, or partial final line.
        ensure_path_matches_open_file(&self.file, &self.path)?;
        let mut digest_file = self.file.try_clone()?;
        digest_file.seek(SeekFrom::Start(0))?;
        let mut sidecar_content = Vec::new();
        std::io::Read::by_ref(&mut digest_file)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut sidecar_content)?;
        let opened_file_bytes = digest_file.metadata()?.len();
        ensure_path_matches_open_file(&self.file, &self.path)?;
        let sidecar_bytes = sidecar_content.len() as u64;
        if sidecar_bytes != self.bytes_written || opened_file_bytes != sidecar_bytes {
            return Err(invalid(
                "sidecar byte length does not match bytes recorded by the writer",
            ));
        }
        if sidecar_content.last() != Some(&b'\n') {
            return Err(invalid("sidecar must end with a complete JSONL record"));
        }
        let mut validated_events = 0_u64;
        for line in sidecar_content
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let value: serde_json::Value = serde_json::from_slice(line)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if !value.is_object() {
                return Err(invalid("sidecar JSONL records must be JSON objects"));
            }
            validated_events += 1;
        }
        if validated_events != self.event_count {
            return Err(invalid(
                "sidecar JSONL record count does not match the writer event count",
            ));
        }

        let sidecar_sha256 = Sha256::digest(&sidecar_content)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let sidecar_file = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid("sidecar filename must be valid UTF-8"))?
            .to_string();

        let mut connection_subscriptions = self
            .connections
            .values()
            .map(|state| {
                let mut subscriptions = state.identity.subscriptions.clone();
                subscriptions.sort();
                ConnectionSubscription {
                    connection_id: state.identity.connection_id.clone(),
                    batch_id: state.identity.batch_id,
                    route: state.identity.route,
                    subscription_set_sha256: subscription_set_sha256(&subscriptions),
                    subscriptions,
                }
            })
            .collect::<Vec<_>>();
        connection_subscriptions.sort_by(|left, right| {
            (left.batch_id, left.route, &left.connection_id).cmp(&(
                right.batch_id,
                right.route,
                &right.connection_id,
            ))
        });

        let manifest = CaptureManifest {
            schema_version: 1,
            source_commit: self.source_commit.clone(),
            session_id: self.session_id.clone(),
            sidecar_file,
            sidecar_bytes,
            sidecar_sha256,
            capture_finalized: true,
            capture_complete_through_ms,
            finalized_at_ms,
            event_count: self.event_count,
            connection_subscriptions,
        };

        let manifest_path = Self::manifest_path(&self.path);
        let mut temp_name = manifest_path
            .file_name()
            .ok_or_else(|| invalid("manifest path has no filename"))?
            .to_os_string();
        temp_name.push(format!(
            ".tmp-{}-{}",
            std::process::id(),
            self.next_event_sequence
        ));
        let temp_path = manifest_path.with_file_name(temp_name);
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if manifest_bytes.len() > MAX_MANIFEST_BYTES {
            return Err(invalid(
                "serialized capture manifest exceeds the 1 MiB bound",
            ));
        }

        // Finalization was made terminal before the first filesystem operation.
        let publish_result = (|| -> io::Result<()> {
            let mut temp_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            temp_file.write_all(&manifest_bytes)?;
            temp_file.flush()?;
            temp_file.sync_all()?;
            fs::hard_link(&temp_path, &manifest_path)?;
            fs::remove_file(&temp_path)?;
            sync_parent_directory(&manifest_path)?;
            Ok(())
        })();
        if publish_result.is_err() {
            let _ = fs::remove_file(&temp_path);
            return publish_result.map(|_| manifest);
        }

        Ok(manifest)
    }

    fn ensure_usable(&self) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "sidecar writer is poisoned after an I/O failure or finalization",
            ));
        }
        Ok(())
    }

    fn ensure_connecting(&self, identity: &ConnectionIdentity) -> io::Result<&ConnectionState> {
        self.ensure_usable()?;
        validate_identity(identity)?;
        let state = self
            .connections
            .get(&identity.connection_id)
            .ok_or_else(|| invalid("event references an unknown connection_id"))?;
        if state.identity != *identity {
            return Err(invalid(
                "connection_id reused with a different batch, route, or subscription set",
            ));
        }
        if state.lifecycle != Lifecycle::Connecting {
            return Err(invalid("event requires a pending connection attempt"));
        }
        Ok(state)
    }

    fn ensure_connected(&self, identity: &ConnectionIdentity) -> io::Result<&ConnectionState> {
        self.ensure_usable()?;
        validate_identity(identity)?;
        let state = self
            .connections
            .get(&identity.connection_id)
            .ok_or_else(|| invalid("event references an unknown connection_id"))?;
        if state.identity != *identity {
            return Err(invalid(
                "connection_id reused with a different batch, route, or subscription set",
            ));
        }
        if state.lifecycle != Lifecycle::Connected {
            return Err(invalid("event requires an open WebSocket connection"));
        }
        Ok(state)
    }

    fn ensure_disconnected(&self, identity: &ConnectionIdentity) -> io::Result<&ConnectionState> {
        self.ensure_usable()?;
        validate_identity(identity)?;
        let state = self
            .connections
            .get(&identity.connection_id)
            .ok_or_else(|| invalid("event references an unknown connection_id"))?;
        if state.identity != *identity {
            return Err(invalid(
                "connection_id reused with a different batch, route, or subscription set",
            ));
        }
        if state.lifecycle != Lifecycle::Disconnected {
            return Err(invalid("reconnect schedule requires a disconnected socket"));
        }
        Ok(state)
    }

    #[allow(clippy::too_many_arguments)]
    fn append_event(
        &mut self,
        identity: &ConnectionIdentity,
        event_type: &'static str,
        event_at_ms: u64,
        heartbeat_id: Option<&str>,
        reason: Option<&'static str>,
        attempt: Option<u32>,
        delay_ms: Option<u64>,
        subscription_count: Option<usize>,
        subscription_set_sha256: Option<String>,
    ) -> io::Result<()> {
        self.ensure_usable()?;
        if self.event_count >= MAX_EVENTS {
            return Err(invalid("sidecar event count limit exceeded"));
        }
        if event_at_ms > MAX_TIMESTAMP_MS {
            return Err(invalid(
                "event timestamp exceeds the signed 64-bit contract",
            ));
        }
        if self.last_event_at_ms.is_some_and(|last| event_at_ms < last) {
            return Err(invalid("event timestamps must be globally non-decreasing"));
        }

        let event = Event {
            schema_version: 1,
            event_id: format!("{}:{}", self.session_id, self.next_event_sequence),
            source_commit: &self.source_commit,
            session_id: &self.session_id,
            connection_id: &identity.connection_id,
            batch_id: identity.batch_id,
            route: identity.route,
            event_type,
            event_at_ms,
            heartbeat_id,
            reason,
            attempt,
            delay_ms,
            subscription_count,
            subscription_set_sha256,
        };
        let mut line = serde_json::to_vec(&event)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        if line.len() > MAX_EVENT_BYTES {
            return Err(invalid("serialized sidecar event exceeds 16 KiB"));
        }
        if self.bytes_written.saturating_add(line.len() as u64) > MAX_FILE_BYTES {
            return Err(invalid("sidecar file size limit exceeded"));
        }

        if let Err(error) = self
            .file
            .write_all(&line)
            .and_then(|_| self.file.flush())
            .and_then(|_| self.file.sync_data())
        {
            self.poisoned = true;
            return Err(error);
        }

        self.bytes_written += line.len() as u64;
        self.event_count += 1;
        self.next_event_sequence += 1;
        self.last_event_at_ms = Some(event_at_ms);
        Ok(())
    }
}

/// Create the sidecar exclusively and sync its parent directory before returning it.
/// The sync callback is injectable so the create/cleanup failure boundary can be tested.
fn create_sidecar_file(
    path: &Path,
    sync_parent: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .create_new(true)
        .open(path)?;
    sync_parent(path)?;
    Ok(file)
}

#[cfg(unix)]
fn ensure_path_matches_open_file(file: &File, path: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let open_metadata = file.metadata()?;
    let path_metadata = fs::symlink_metadata(path)?;
    if path_metadata.file_type().is_symlink()
        || !path_metadata.is_file()
        || open_metadata.dev() != path_metadata.dev()
        || open_metadata.ino() != path_metadata.ino()
    {
        return Err(invalid(
            "sidecar path no longer identifies the writer's open file",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_path_matches_open_file(file: &File, path: &Path) -> io::Result<()> {
    let open_metadata = file.metadata()?;
    let path_metadata = fs::symlink_metadata(path)?;
    if path_metadata.file_type().is_symlink()
        || !path_metadata.is_file()
        || open_metadata.len() != path_metadata.len()
    {
        return Err(invalid(
            "sidecar path no longer matches the writer's open file",
        ));
    }
    Ok(())
}

fn validate_identity(identity: &ConnectionIdentity) -> io::Result<()> {
    if identity.connection_id.is_empty() || identity.connection_id.len() > 128 {
        return Err(invalid("connection_id must contain 1..=128 bytes"));
    }
    if identity.batch_id > MAX_BATCH_ID {
        return Err(invalid("batch_id exceeds the supported bound"));
    }
    if identity.subscriptions.is_empty() || identity.subscriptions.len() > MAX_SUBSCRIPTIONS {
        return Err(invalid(
            "subscription set is empty or exceeds the supported count",
        ));
    }
    let mut unique = HashSet::new();
    let mut total_bytes = 0_usize;
    for subscription in &identity.subscriptions {
        if subscription.is_empty() || subscription.len() > MAX_SUBSCRIPTION_BYTES {
            return Err(invalid("subscription name is empty or exceeds 256 bytes"));
        }
        if !unique.insert(subscription) {
            return Err(invalid("subscription set contains a duplicate stream"));
        }
        total_bytes = total_bytes.saturating_add(subscription.len());
    }
    if total_bytes > 2 * 1024 * 1024 {
        return Err(invalid("subscription set exceeds the 2 MiB bound"));
    }
    Ok(())
}

fn subscription_set_sha256(subscriptions: &[String]) -> String {
    let mut sorted = subscriptions.to_vec();
    sorted.sort();
    let mut hasher = Sha256::new();
    for subscription in sorted {
        let bytes = subscription.as_bytes();
        hasher.update((bytes.len() as u32).to_be_bytes());
        hasher.update(bytes);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    // Directory handles cannot be synced portably through std on every target.
    Ok(())
}

/// Read-only classification of a captured connection-sidecar file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureValidation {
    UnsealedValid {
        sidecar_bytes: u64,
        event_count: u64,
    },
    TruncatedFinalLine {
        line_number: usize,
    },
    MalformedJsonl {
        line_number: usize,
    },
    ManifestMismatch {
        reason: String,
    },
    SealedConsistent {
        event_count: u64,
        sidecar_bytes: u64,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestValidationFields {
    schema_version: u8,
    sidecar_file: String,
    sidecar_bytes: u64,
    sidecar_sha256: String,
    capture_finalized: bool,
    event_count: u64,
}

/// Validate a sidecar and its optional manifest without modifying either file.
pub fn validate_capture(path: impl AsRef<Path>) -> io::Result<CaptureValidation> {
    let path = path.as_ref();
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("sidecar path must be a regular, non-symlink file"));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(invalid("sidecar file size limit exceeded"));
    }

    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sidecar changed while the validator was reading its bounded snapshot",
        ));
    }

    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        let line_number = bytes.iter().filter(|byte| **byte == b'\n').count() + 1;
        return Ok(CaptureValidation::TruncatedFinalLine { line_number });
    }

    let mut event_count = 0_u64;
    let record_count = bytes.iter().filter(|byte| **byte == b'\n').count();
    for (index, line) in bytes
        .split(|byte| *byte == b'\n')
        .take(record_count)
        .enumerate()
    {
        let value: serde_json::Value = match serde_json::from_slice(line) {
            Ok(value) => value,
            Err(_) => {
                return Ok(CaptureValidation::MalformedJsonl {
                    line_number: index + 1,
                });
            }
        };
        if !value.is_object() {
            return Ok(CaptureValidation::MalformedJsonl {
                line_number: index + 1,
            });
        }
        event_count += 1;
        if event_count > MAX_EVENTS {
            return Err(invalid("sidecar event count limit exceeded"));
        }
    }

    let manifest_path = SidecarWriter::manifest_path(path);
    let manifest_metadata = match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(CaptureValidation::UnsealedValid {
                sidecar_bytes: bytes.len() as u64,
                event_count,
            });
        }
        Err(error) => return Err(error),
    };
    if manifest_metadata.file_type().is_symlink()
        || !manifest_metadata.is_file()
        || manifest_metadata.len() > 1024 * 1024
    {
        return Ok(CaptureValidation::ManifestMismatch {
            reason: "manifest is not a bounded regular file".to_string(),
        });
    }

    let manifest_bytes = fs::read(&manifest_path)?;
    let manifest: ManifestValidationFields = match serde_json::from_slice(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            return Ok(CaptureValidation::ManifestMismatch {
                reason: format!("manifest JSON is invalid: {error}"),
            });
        }
    };
    let expected_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let mismatch = if manifest.schema_version != 1 {
        Some("unsupported manifest schema version")
    } else if !manifest.capture_finalized {
        Some("manifest does not declare a finalized capture")
    } else if manifest.sidecar_file != expected_name {
        Some("manifest sidecar filename does not match the validated file")
    } else if manifest.sidecar_bytes != bytes.len() as u64 {
        Some("manifest sidecar byte count does not match")
    } else if manifest.sidecar_sha256 != digest {
        Some("manifest sidecar SHA-256 does not match")
    } else if manifest.event_count != event_count {
        Some("manifest event count does not match JSONL records")
    } else {
        None
    };
    if let Some(reason) = mismatch {
        return Ok(CaptureValidation::ManifestMismatch {
            reason: reason.to_string(),
        });
    }

    Ok(CaptureValidation::SealedConsistent {
        event_count,
        sidecar_bytes: bytes.len() as u64,
    })
}

#[cfg(test)]
mod create_cleanup_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn removes_new_sidecar_when_parent_directory_sync_fails() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "t1-sidecar-create-sync-failure-{}-{nonce}.jsonl",
            std::process::id()
        ));

        let result = create_sidecar_file(&path, |_| {
            Err(io::Error::other("injected parent-directory sync failure"))
        });

        assert!(result.is_err(), "injected sync failure must be returned");
        assert!(
            !path.exists(),
            "failed creation must not leave an orphan sidecar path"
        );
    }
}
