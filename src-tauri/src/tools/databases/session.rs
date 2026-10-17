//! Postgres sessions: connection, read-only mode, bounded queries, cancellation.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::Serialize;
use tokio::sync::Notify;
use tokio_postgres::error::{ErrorPosition, SqlState};
use tokio_postgres::{CancelToken, Client, Config, NoTls, SimpleQueryMessage};
use tokio_postgres_rustls::MakeRustlsConnect;

use super::results::{Assembler, Message, RunOutput, Step};
use super::url::SslMode;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const BARRIER_TIMEOUT: Duration = Duration::from_secs(5);
/// Time a cancelled run has to end before it is abandoned (R14).
pub const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// Error of a read-only toggle refused because a transaction is open.
pub const TRANSACTION_OPEN: &str = "Commit or roll back the open transaction first.";

pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: String,
    pub ssl_mode: SslMode,
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Target")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field(
                "password",
                &self.password.as_ref().map(|_| format_args!("<redacted>")),
            )
            .field("database", &self.database)
            .field("ssl_mode", &self.ssl_mode)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunError {
    Connect {
        host: String,
        port: u16,
        cause: String,
    },
    Closed,
    Db {
        message: String,
        code: String,
        hint: Option<String>,
        position: Option<u32>,
    },
    Cancelled,
    Other {
        message: String,
    },
}

pub fn connect_message(host: &str, port: u16, cause: &str) -> String {
    let mut message = format!("Could not connect to {host}:{port}. {cause}");
    let local = matches!(host, "127.0.0.1" | "localhost" | "::1");
    if local && port != 5432 {
        message.push_str(" Is the tunnel running?");
    }
    message
}

pub struct Session {
    client: Client,
    cancel: CancelToken,
    ssl_mode: SslMode,
    /// Set by `CancelHandle::cancel`, reset at the start of each `run`.
    user_cancelled: Arc<AtomicBool>,
    /// Set when the connection may not be ready for a new query; the caller
    /// then sees the session as closed and reconnects.
    broken: Arc<AtomicBool>,
    /// Ends the pending `run` with `Cancelled`, used by the cancel watchdog.
    abort: Arc<Notify>,
    /// Number of runs that have ended, to tell a run from the next one.
    finished_runs: Arc<AtomicU64>,
}

pub struct CancelHandle {
    token: CancelToken,
    ssl_mode: SslMode,
    user_cancelled: Arc<AtomicBool>,
    watch: RunWatch,
}

/// The run a `CancelHandle` was taken for.
#[derive(Clone)]
struct RunWatch {
    abort: Arc<Notify>,
    broken: Arc<AtomicBool>,
    finished_runs: Arc<AtomicU64>,
    /// Value of `finished_runs` when the handle was taken: while it is
    /// unchanged, that run has not ended.
    run: u64,
}

impl RunWatch {
    /// Abandons the run if it has not ended: the session is marked broken
    /// (the next run reconnects) and the pending `run` returns `Cancelled`.
    fn abort_if_running(&self) -> bool {
        if self.finished_runs.load(Ordering::SeqCst) != self.run {
            return false;
        }
        self.broken.store(true, Ordering::SeqCst);
        self.abort.notify_waiters();
        true
    }
}

impl CancelHandle {
    /// Asks the server to cancel the running query. Errors are ignored: the
    /// query may already be over.
    pub async fn cancel(&self) {
        self.user_cancelled.store(true, Ordering::SeqCst);
        send_cancel(&self.token, self.ssl_mode).await;
    }

    /// If the run has not ended `grace` from now, abandons it. Through a
    /// half-open tunnel neither the query nor the cancel gets an answer, and
    /// the run would otherwise wait forever.
    pub fn arm_watchdog(&self, grace: Duration) {
        let watch = self.watch.clone();
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            if watch.abort_if_running() {
                eprintln!("LsDev databases: cancelled query did not end, session dropped");
            }
        });
    }
}

async fn send_cancel(token: &CancelToken, ssl_mode: SslMode) {
    let _ = match ssl_mode {
        SslMode::Disable => token.cancel_query(NoTls).await,
        SslMode::Prefer | SslMode::Require => match rustls_connector() {
            Ok(tls) => token.cancel_query(tls).await,
            Err(_) => return,
        },
    };
}

pub async fn connect(target: &Target, read_only: bool) -> Result<Session, RunError> {
    // The timeout covers the read-only setup too.
    tokio::time::timeout(CONNECT_TIMEOUT, open(target, read_only))
        .await
        .map_err(|_| RunError::Connect {
            host: target.host.clone(),
            port: target.port,
            cause: "Timed out after 10 s.".to_string(),
        })?
}

async fn open(target: &Target, read_only: bool) -> Result<Session, RunError> {
    // Built field by field, never from a URL string, so no error can echo a URL.
    let mut config = Config::new();
    config
        .host(&target.host)
        .port(target.port)
        .user(&target.user)
        .dbname(&target.database)
        .connect_timeout(CONNECT_TIMEOUT)
        .application_name("LsDev");
    if let Some(password) = &target.password {
        config.password(password);
    }

    let connect_error = |cause: String| RunError::Connect {
        host: target.host.clone(),
        port: target.port,
        cause,
    };

    let client = match target.ssl_mode {
        SslMode::Disable => {
            config.ssl_mode(tokio_postgres::config::SslMode::Disable);
            let (client, connection) = config
                .connect(NoTls)
                .await
                .map_err(|e| connect_failure(e, &connect_error))?;
            tokio::spawn(connection);
            client
        }
        SslMode::Prefer | SslMode::Require => {
            config.ssl_mode(if target.ssl_mode == SslMode::Require {
                tokio_postgres::config::SslMode::Require
            } else {
                tokio_postgres::config::SslMode::Prefer
            });
            let tls = rustls_connector()?;
            let (client, connection) = config
                .connect(tls)
                .await
                .map_err(|e| connect_failure(e, &connect_error))?;
            tokio::spawn(connection);
            client
        }
    };

    let session = Session {
        cancel: client.cancel_token(),
        client,
        ssl_mode: target.ssl_mode,
        user_cancelled: Arc::new(AtomicBool::new(false)),
        broken: Arc::new(AtomicBool::new(false)),
        abort: Arc::new(Notify::new()),
        finished_runs: Arc::new(AtomicU64::new(0)),
    };
    session.set_default_read_only(read_only).await?;
    Ok(session)
}

impl Session {
    pub fn is_closed(&self) -> bool {
        self.client.is_closed() || self.broken.load(Ordering::SeqCst)
    }

    async fn set_default_read_only(&self, on: bool) -> Result<(), RunError> {
        let value = if on { "on" } else { "off" };
        self.client
            .simple_query(&format!("SET default_transaction_read_only = {value}"))
            .await
            .map(|_| ())
            .map_err(|e| map_error(e, false))
    }

    async fn show(&self, setting: &str) -> Result<String, RunError> {
        let messages = self
            .client
            .simple_query(&format!("SHOW {setting}"))
            .await
            .map_err(|e| map_error(e, false))?;
        messages
            .iter()
            .find_map(|m| match m {
                SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
                _ => None,
            })
            .ok_or_else(|| RunError::Other {
                message: format!("SHOW {setting} returned no value."),
            })
    }

    /// Sets the read-only default and checks that it applies. Inside an open
    /// transaction the new default does not apply, and a ROLLBACK would undo
    /// it: the previous value is restored and the toggle is refused.
    pub async fn set_read_only(&self, on: bool) -> Result<(), RunError> {
        let previous = self.show("default_transaction_read_only").await?;
        self.set_default_read_only(on).await?;
        // A separate query: outside a transaction it starts a new implicit
        // one, which takes the new default.
        let current = self.show("transaction_read_only").await?;
        if setting_is(&current, on) {
            return Ok(());
        }
        self.set_default_read_only(setting_is(&previous, true))
            .await?;
        Err(RunError::Other {
            message: TRANSACTION_OPEN.to_string(),
        })
    }

    pub fn cancel_token(&self) -> CancelHandle {
        CancelHandle {
            token: self.cancel.clone(),
            ssl_mode: self.ssl_mode,
            user_cancelled: Arc::clone(&self.user_cancelled),
            watch: RunWatch {
                abort: Arc::clone(&self.abort),
                broken: Arc::clone(&self.broken),
                finished_runs: Arc::clone(&self.finished_runs),
                run: self.finished_runs.load(Ordering::SeqCst),
            },
        }
    }

    /// Runs `sql`. Take the `CancelHandle` before calling, so it refers to
    /// this run.
    pub async fn run(&self, sql: &str) -> Result<RunOutput, RunError> {
        if self.is_closed() {
            return Err(RunError::Closed);
        }
        let result = tokio::select! {
            r = self.run_query(sql) => r,
            _ = self.abort.notified() => Err(RunError::Cancelled),
        };
        self.finished_runs.fetch_add(1, Ordering::SeqCst);
        result
    }

    async fn run_query(&self, sql: &str) -> Result<RunOutput, RunError> {
        self.user_cancelled.store(false, Ordering::SeqCst);
        let cancelled = || self.user_cancelled.load(Ordering::SeqCst);
        let started = Instant::now();
        let mut assembler = Assembler::new();
        let stream = self
            .client
            .simple_query_raw(sql)
            .await
            .map_err(|e| map_error(e, cancelled()))?;
        let mut stream = Box::pin(stream);
        let mut stopped = false;
        while let Some(item) = stream.next().await {
            let message = match item.map_err(|e| map_error(e, cancelled()))? {
                SimpleQueryMessage::RowDescription(cols) => {
                    Message::Columns(cols.iter().map(|c| c.name().to_string()).collect())
                }
                SimpleQueryMessage::Row(row) => Message::Row(
                    (0..row.len())
                        .map(|i| row.get(i).map(str::to_string))
                        .collect(),
                ),
                SimpleQueryMessage::CommandComplete(n) => Message::Done(n),
                _ => continue,
            };
            if assembler.push(message) == Step::Stop {
                stopped = true;
                break;
            }
        }
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if !stopped {
            return Ok(assembler.finish(elapsed_ms));
        }

        drop(stream);
        send_cancel(&self.cancel, self.ssl_mode).await;
        // Barrier: wait until the connection has drained the cancelled query,
        // so the next query starts on a ready connection. Postgres does not
        // acknowledge cancels, so a late cancel can, rarely, still hit the
        // next query.
        let mut failed_in_tx = false;
        match tokio::time::timeout(BARRIER_TIMEOUT, self.client.simple_query("select 1")).await {
            Err(_) => self.broken.store(true, Ordering::SeqCst),
            Ok(Err(e)) if e.code() == Some(&SqlState::IN_FAILED_SQL_TRANSACTION) => {
                failed_in_tx = true;
                // The cancel aborted an explicit transaction: end it so the
                // session can run queries again.
                match tokio::time::timeout(BARRIER_TIMEOUT, self.client.simple_query("ROLLBACK"))
                    .await
                {
                    Ok(Ok(_)) => {}
                    _ => self.broken.store(true, Ordering::SeqCst),
                }
            }
            Ok(_) => {}
        }
        let mut out = assembler.finish(elapsed_ms);
        // The truncated statement is the last summary; the ones before it
        // ran in the same implicit transaction, undone by the cancel.
        let before_stop = out.statements.len().saturating_sub(1);
        out.rolled_back = batch_rolled_back(before_stop, failed_in_tx);
        Ok(out)
    }
}

/// Whether a boolean setting shown by Postgres (`on` / `off`) equals `on`.
fn setting_is(shown: &str, on: bool) -> bool {
    shown.trim().eq_ignore_ascii_case("on") == on
}

/// Whether stopping at the row limit undid earlier work: statements that
/// completed before the truncated one share its implicit transaction, and a
/// failed explicit transaction loses everything since `BEGIN`.
fn batch_rolled_back(statements_before_stop: usize, barrier_failed_in_tx: bool) -> bool {
    statements_before_stop > 0 || barrier_failed_in_tx
}

/// `57014` means "Query cancelled" only when the user asked for it; a
/// `statement_timeout` raises the same code and stays a database error.
fn is_user_cancel(code: &SqlState, user_cancelled: bool) -> bool {
    *code == SqlState::QUERY_CANCELED && user_cancelled
}

fn map_error(e: tokio_postgres::Error, user_cancelled: bool) -> RunError {
    if let Some(db) = e.as_db_error() {
        if is_user_cancel(db.code(), user_cancelled) {
            return RunError::Cancelled;
        }
        return RunError::Db {
            message: db.message().to_string(),
            code: db.code().code().to_string(),
            hint: db.hint().map(str::to_string),
            position: match db.position() {
                Some(ErrorPosition::Original(n)) => Some(*n),
                _ => None,
            },
        };
    }
    if e.is_closed() {
        return RunError::Closed;
    }
    RunError::Other {
        message: e.to_string(),
    }
}

/// Server-side errors (bad password, unknown database) keep their Postgres
/// message; transport errors become `Connect` with the underlying cause.
fn connect_failure(
    e: tokio_postgres::Error,
    connect_error: &dyn Fn(String) -> RunError,
) -> RunError {
    if e.as_db_error().is_some() {
        return map_error(e, false);
    }
    let cause = match std::error::Error::source(&e) {
        Some(source) => source.to_string(),
        None => e.to_string(),
    };
    connect_error(sentence(&cause))
}

/// Capitalises the first letter and ends with a period.
fn sentence(s: &str) -> String {
    let mut chars = s.trim().chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => return String::new(),
    };
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

fn rustls_connector() -> Result<MakeRustlsConnect, RunError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| RunError::Other {
            message: e.to_string(),
        })?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate(provider)))
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

/// Accepts any server certificate. This follows libpq's `prefer` and
/// `require` modes, which encrypt the connection without verifying the
/// certificate: development databases and tunnels use self-signed or
/// mismatched certificates. Handshake signatures are still checked, so the
/// server must hold the key of the certificate it presents.
/// `verify-ca` / `verify-full` are rejected by `parse_url`, so no verifying mode reaches this.
#[derive(Debug)]
struct AcceptAnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_hint_only_for_local_non_default_port() {
        assert!(connect_message("127.0.0.1", 5433, "Connection refused.")
            .ends_with("Is the tunnel running?"));
        assert!(connect_message("localhost", 6543, "Connection refused.")
            .ends_with("Is the tunnel running?"));
        assert!(
            connect_message("::1", 6543, "Connection refused.").ends_with("Is the tunnel running?")
        );
        assert_eq!(
            connect_message("127.0.0.1", 5432, "Connection refused."),
            "Could not connect to 127.0.0.1:5432. Connection refused."
        );
        assert!(!connect_message("db.example.com", 5433, "Connection refused.").contains("tunnel"));
    }

    #[test]
    fn setting_is_compares_on_and_off() {
        assert!(setting_is("on", true));
        assert!(setting_is("ON", true));
        assert!(setting_is("off", false));
        assert!(!setting_is("off", true));
        assert!(!setting_is("on", false));
    }

    #[test]
    fn rolled_back_when_earlier_statements_or_failed_transaction() {
        assert!(!batch_rolled_back(0, false));
        assert!(batch_rolled_back(1, false));
        assert!(batch_rolled_back(0, true));
        assert!(batch_rolled_back(2, true));
    }

    #[test]
    fn query_canceled_is_cancelled_only_when_the_user_asked() {
        assert!(is_user_cancel(&SqlState::QUERY_CANCELED, true));
        assert!(!is_user_cancel(&SqlState::QUERY_CANCELED, false));
        assert!(!is_user_cancel(&SqlState::READ_ONLY_SQL_TRANSACTION, true));
    }

    #[test]
    fn debug_hides_password() {
        let t = Target {
            host: "h".into(),
            port: 5432,
            user: "u".into(),
            password: Some("s3cr3t-pw".into()),
            database: "d".into(),
            ssl_mode: SslMode::Disable,
        };
        let s = format!("{t:?}");
        assert!(!s.contains("s3cr3t-pw"), "{s}");
        assert!(s.contains("<redacted>"), "{s}");
    }

    #[test]
    fn run_error_serialized_shapes() {
        let j = |e: RunError| serde_json::to_value(e).unwrap();
        assert_eq!(
            j(RunError::Connect {
                host: "h".into(),
                port: 5433,
                cause: "c".into()
            }),
            serde_json::json!({"kind": "connect", "host": "h", "port": 5433, "cause": "c"})
        );
        assert_eq!(j(RunError::Closed), serde_json::json!({"kind": "closed"}));
        assert_eq!(
            j(RunError::Db {
                message: "m".into(),
                code: "25006".into(),
                hint: None,
                position: Some(3)
            }),
            serde_json::json!({"kind": "db", "message": "m", "code": "25006", "hint": null, "position": 3})
        );
        assert_eq!(
            j(RunError::Cancelled),
            serde_json::json!({"kind": "cancelled"})
        );
        assert_eq!(
            j(RunError::Other {
                message: "m".into()
            }),
            serde_json::json!({"kind": "other", "message": "m"})
        );
    }

    fn watch(finished: u64, run: u64) -> RunWatch {
        RunWatch {
            abort: Arc::new(Notify::new()),
            broken: Arc::new(AtomicBool::new(false)),
            finished_runs: Arc::new(AtomicU64::new(finished)),
            run,
        }
    }

    #[test]
    fn watchdog_leaves_an_ended_run_alone() {
        let w = watch(4, 3);
        assert!(!w.abort_if_running());
        assert!(!w.broken.load(Ordering::SeqCst));
    }

    #[test]
    fn watchdog_marks_a_pending_run_broken() {
        let w = watch(3, 3);
        assert!(w.abort_if_running());
        assert!(w.broken.load(Ordering::SeqCst));
    }

    /// Writes a backend message: type byte, length, body.
    async fn send(sock: &mut tokio::net::TcpStream, kind: u8, body: &[u8]) {
        use tokio::io::AsyncWriteExt;
        let mut m = vec![kind];
        m.extend_from_slice(&((body.len() as u32 + 4).to_be_bytes()));
        m.extend_from_slice(body);
        sock.write_all(&m).await.unwrap();
    }

    /// Reads a frontend message after the startup one: type byte, body.
    async fn receive(sock: &mut tokio::net::TcpStream) -> Option<(u8, Vec<u8>)> {
        use tokio::io::AsyncReadExt;
        let kind = sock.read_u8().await.ok()?;
        let len = sock.read_u32().await.ok()?;
        let mut body = vec![0; len as usize - 4];
        sock.read_exact(&mut body).await.ok()?;
        Some((kind, body))
    }

    /// A fake server that completes the startup and answers the first query
    /// (the read-only SET), then never answers again: a half-open tunnel.
    /// Further connections (the cancel request) are accepted and ignored.
    async fn silent_server() -> u16 {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let len = sock.read_u32().await.unwrap();
            let mut startup = vec![0; len as usize - 4];
            sock.read_exact(&mut startup).await.unwrap();
            send(&mut sock, b'R', &0u32.to_be_bytes()).await;
            send(&mut sock, b'K', &[0, 0, 0, 1, 0, 0, 0, 2]).await;
            send(&mut sock, b'Z', b"I").await;
            let (kind, _) = receive(&mut sock).await.unwrap();
            assert_eq!(kind, b'Q');
            send(&mut sock, b'C', b"SET\0").await;
            send(&mut sock, b'Z', b"I").await;
            // Keep the session socket open, read and ignore everything.
            tokio::spawn(async move { while receive(&mut sock).await.is_some() {} });
            loop {
                let Ok((mut cancel, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0u8; 64];
                let _ = cancel.read(&mut buf).await;
            }
        });
        port
    }

    #[tokio::test]
    async fn watchdog_ends_a_run_the_server_never_answers() {
        let port = silent_server().await;
        let target = Target {
            host: "127.0.0.1".into(),
            port,
            user: "app".into(),
            password: None,
            database: "app".into(),
            ssl_mode: SslMode::Disable,
        };
        let s = Arc::new(connect(&target, false).await.unwrap());
        let handle = s.cancel_token();
        let runner = Arc::clone(&s);
        let task = tokio::spawn(async move { runner.run("select 1").await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.arm_watchdog(Duration::from_millis(200));
        handle.cancel().await;
        let r = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .expect("the run is still stuck")
            .unwrap();
        assert_eq!(r, Err(RunError::Cancelled));
        assert!(s.is_closed());
    }

    #[tokio::test]
    async fn connect_to_closed_port_fails_fast() {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let target = Target {
            host: "127.0.0.1".into(),
            port,
            user: "postgres".into(),
            password: None,
            database: "postgres".into(),
            ssl_mode: SslMode::Prefer,
        };
        let r = tokio::time::timeout(std::time::Duration::from_secs(11), connect(&target, true))
            .await
            .expect("connect did not return within 11 s");
        match r {
            Err(RunError::Connect { host, port: p, .. }) => {
                assert_eq!(host, "127.0.0.1");
                assert_eq!(p, port);
            }
            Err(e) => panic!("expected Connect, got {e:?}"),
            Ok(_) => panic!("expected Connect, got a session"),
        }
    }
}

#[cfg(test)]
mod integration {
    //! Run only when LSDEV_TEST_PG_URL is set; pass trivially otherwise.

    use super::*;
    use crate::tools::databases::url::ParsedUrl;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn target_from(p: &ParsedUrl) -> Target {
        Target {
            host: p.host.clone(),
            port: p.port,
            user: p.user.clone(),
            password: p.password.clone(),
            database: p.database.clone(),
            ssl_mode: p.ssl_mode,
        }
    }

    fn code_of(r: Result<RunOutput, RunError>) -> String {
        match r {
            Err(RunError::Db { code, .. }) => code,
            other => panic!("expected a db error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn select_text_and_null() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = connect(&target, false).await.unwrap();
        let out = s.run("select 1 as a, null as b").await.unwrap();
        let rs = out.result.unwrap();
        assert_eq!(rs.columns, vec!["a", "b"]);
        assert_eq!(rs.rows, vec![vec![Some("1".to_string()), None]]);
        assert!(!rs.truncated);
    }

    #[tokio::test]
    async fn several_statements() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = connect(&target, false).await.unwrap();
        let out = s
            .run("create temp table t(x int); insert into t values (1),(2); select * from t")
            .await
            .unwrap();
        assert_eq!(out.statements.len(), 3);
        assert_eq!(out.result.unwrap().rows.len(), 2);
    }

    #[tokio::test]
    async fn read_only_toggles() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = connect(&target, true).await.unwrap();
        assert_eq!(
            code_of(s.run("create table lsdev_ro_probe(x int)").await),
            "25006"
        );
        s.set_read_only(false).await.unwrap();
        // A regular table: Postgres allows writes to temporary tables in a
        // read-only transaction.
        s.run("drop table if exists lsdev_ro_rw; create table lsdev_ro_rw(x int)")
            .await
            .unwrap();
        s.set_read_only(true).await.unwrap();
        let refused = code_of(s.run("insert into lsdev_ro_rw values (1)").await);
        s.set_read_only(false).await.unwrap();
        s.run("drop table lsdev_ro_rw").await.unwrap();
        assert_eq!(refused, "25006");
    }

    #[tokio::test]
    async fn read_only_toggle_refused_inside_a_transaction() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = connect(&target, false).await.unwrap();
        s.run("begin; select 1").await.unwrap();
        assert_eq!(
            s.set_read_only(true).await,
            Err(RunError::Other {
                message: TRANSACTION_OPEN.into()
            })
        );
        let shown = |out: RunOutput| out.result.unwrap().rows[0][0].clone();
        let out = s.run("show default_transaction_read_only").await.unwrap();
        assert_eq!(shown(out), Some("off".to_string()));
        s.run("rollback").await.unwrap();
        let out = s.run("show default_transaction_read_only").await.unwrap();
        assert_eq!(shown(out), Some("off".to_string()));
        // Outside a transaction the toggle applies.
        s.set_read_only(true).await.unwrap();
        assert_eq!(code_of(s.run("create temp table q(x int)").await), "25006");
    }

    #[tokio::test]
    async fn reconnection_keeps_read_only() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = connect(&target, true).await.unwrap();
        assert!(s
            .run("select pg_terminate_backend(pg_backend_pid())")
            .await
            .is_err());
        let s = connect(&target, true).await.unwrap();
        let out = s.run("show default_transaction_read_only").await.unwrap();
        assert_eq!(out.result.unwrap().rows, vec![vec![Some("on".to_string())]]);
    }

    #[tokio::test]
    async fn row_limit_stops_and_session_stays_usable() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = connect(&target, false).await.unwrap();
        let out = s.run("select generate_series(1, 10000000)").await.unwrap();
        let rs = out.result.unwrap();
        assert_eq!(rs.rows.len(), 1000);
        assert!(rs.truncated);
        s.run("select 2").await.unwrap();
    }

    #[tokio::test]
    async fn row_limit_reports_rolled_back_batches() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = connect(&target, false).await.unwrap();
        let out = s
            .run("create temp table r(x int); insert into r values (1); select generate_series(1, 10000000)")
            .await
            .unwrap();
        assert!(out.rolled_back);
        let out = s
            .run("begin; select generate_series(1,10000000)")
            .await
            .unwrap();
        assert!(out.rolled_back);
        s.run("select 1").await.unwrap();
    }

    #[tokio::test]
    async fn statement_timeout_is_a_db_error() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = connect(&target, false).await.unwrap();
        s.run("set statement_timeout = 100").await.unwrap();
        assert_eq!(code_of(s.run("select pg_sleep(2)").await), "57014");
    }

    #[tokio::test]
    async fn cancel_running_query() {
        let Ok(url) = std::env::var("LSDEV_TEST_PG_URL") else {
            return;
        };
        let target = target_from(&crate::tools::databases::url::parse_url(&url).unwrap());
        let s = Arc::new(connect(&target, false).await.unwrap());
        let handle = s.cancel_token();
        let runner = Arc::clone(&s);
        let started = Instant::now();
        let task = tokio::spawn(async move { runner.run("select pg_sleep(10)").await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.cancel().await;
        let r = task.await.unwrap();
        assert!(matches!(r, Err(RunError::Cancelled)), "{r:?}");
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
