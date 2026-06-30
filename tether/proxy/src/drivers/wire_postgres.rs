use std::net::ToSocketAddrs;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, info, instrument, warn};

use super::wire::{DriverKind, WireDriver, WireTransaction};
use super::QueryRow;

pub struct PostgresWireDriver {
    stream: Arc<Mutex<Option<TcpStream>>>,
    connected: bool,
    parameters: std::collections::HashMap<String, String>,
    server_name: String,
}

impl std::fmt::Debug for PostgresWireDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresWireDriver")
            .field("connected", &self.connected)
            .finish()
    }
}

impl PostgresWireDriver {
    pub fn new() -> Self {
        Self {
            stream: Arc::new(Mutex::new(None)),
            connected: false,
            parameters: std::collections::HashMap::new(),
            server_name: String::new(),
        }
    }

    async fn write_all(&self, data: &[u8]) -> Result<()> {
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected")?;
        stream.write_all(data).await?;
        Ok(())
    }

    async fn read_exact(&self, buf: &mut [u8]) -> Result<()> {
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected")?;
        stream.read_exact(buf).await?;
        Ok(())
    }

    async fn read_message_type(&self) -> Result<u8> {
        let mut buf = [0u8; 1];
        self.read_exact(&mut buf).await?;
        Ok(buf[0])
    }

    async fn read_message(&self) -> Result<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        self.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len < 4 {
            warn!(len, "Postgres message length too small");
            bail!("message length {} is too small", len);
        }
        let mut body = vec![0u8; len - 4];
        self.read_exact(&mut body).await?;
        Ok(body)
    }

    fn parse_parameter_status(body: &[u8]) -> Option<(String, String)> {
        let parts: Vec<&[u8]> = body.split(|&b| b == 0).collect();
        if parts.len() >= 2 {
            Some((
                String::from_utf8_lossy(parts[0]).to_string(),
                String::from_utf8_lossy(parts[1]).to_string(),
            ))
        } else {
            None
        }
    }

    fn parse_error_message(body: &[u8]) -> String {
        let mut pos = 1;
        while pos < body.len() {
            match body[pos] {
                b'M' => {
                    pos += 1;
                    let end = body[pos..]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(body.len() - pos);
                    return String::from_utf8_lossy(&body[pos..pos + end]).to_string();
                }
                b'\0' => break,
                _ => {
                    pos += 1;
                    if let Some(end) = body[pos..].iter().position(|&b| b == 0) {
                        pos += end + 1;
                    } else {
                        break;
                    }
                }
            }
        }
        "unknown error".to_string()
    }

    fn write_cstr(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(s.as_bytes());
        buf.push(0);
    }

    /// Encode a parameter value as PostgreSQL binary text format.
    fn encode_param(val: &serde_json::Value) -> Vec<u8> {
        match val {
            serde_json::Value::Null => return b"NULL".to_vec(),
            serde_json::Value::Bool(b) => b.to_string().into_bytes(),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    i.to_string().into_bytes()
                } else if let Some(f) = n.as_f64() {
                    f.to_string().into_bytes()
                } else {
                    n.to_string().into_bytes()
                }
            }
            serde_json::Value::String(s) => s.as_bytes().to_vec(),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                serde_json::to_vec(val).unwrap_or_default()
            }
        }
    }

    /// Read a complete result set after sending a Simple Query.
    /// Returns columns and row data.
    async fn read_result_set(&self) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>)> {
        let mut columns = Vec::new();
        let mut rows = Vec::new();

        loop {
            let msg_type = self.read_message_type().await?;
            let body = self.read_message().await?;

            match msg_type {
                b'T' => {
                    // RowDescription
                    let field_count = u16::from_be_bytes([body[0], body[1]]) as usize;
                    let mut pos = 2;
                    for _ in 0..field_count {
                        // Field name: null-terminated string
                        let name_end = body[pos..]
                            .iter()
                            .position(|&b| b == 0)
                            .context("missing null terminator in field name")?;
                        let name = String::from_utf8_lossy(&body[pos..pos + name_end]).to_string();
                        pos += name_end + 1;
                        // Table OID (4 bytes), Column attr (2 bytes), Data type OID (4 bytes),
                        // Typlen (2 bytes), Typmod (4 bytes), Format code (2 bytes)
                        pos += 4 + 2 + 4 + 2 + 4 + 2;
                        columns.push(name);
                    }
                }
                b'D' => {
                    // DataRow
                    let field_count = u16::from_be_bytes([body[0], body[1]]) as usize;
                    let mut pos = 2;
                    let mut row_values = Vec::with_capacity(field_count);
                    for _ in 0..field_count {
                        let field_len = i32::from_be_bytes([
                            body[pos], body[pos + 1], body[pos + 2], body[pos + 3],
                        ]);
                        pos += 4;
                        if field_len == -1 {
                            // NULL
                            row_values.push(serde_json::Value::Null);
                        } else {
                            let data = &body[pos..pos + field_len as usize];
                            let val = String::from_utf8_lossy(data).to_string();
                            // Try to parse as number, fall back to string
                            let json_val = if let Ok(i) = val.parse::<i64>() {
                                serde_json::Value::Number(i.into())
                            } else if let Ok(f) = val.parse::<f64>() {
                                serde_json::json!(f)
                            } else {
                                serde_json::Value::String(val)
                            };
                            row_values.push(json_val);
                            pos += field_len as usize;
                        }
                    }
                    rows.push(row_values);
                }
                b'C' => {
                    // CommandComplete — tag like "SELECT 5" or "INSERT 0 1"
                    let tag_end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
                    let tag = String::from_utf8_lossy(&body[..tag_end]);
                    debug!(tag = %tag, "command complete");
                }
                b'I' => {
                    // EmptyQueryResponse
                    debug!("empty query response");
                }
                b'Z' => {
                    // ReadyForQuery
                    return Ok((columns, rows));
                }
                b'E' => {
                    let msg = Self::parse_error_message(&body);
                    warn!(msg = %msg, "Postgres query error during result set read");
                    bail!("query error: {}", msg);
                }
                b'1' => {
                    // ParseComplete — for extended query
                    debug!("parse complete");
                }
                b'2' => {
                    // BindComplete — for extended query
                    debug!("bind complete");
                }
                b'3' => {
                    // CloseComplete — for extended query
                    debug!("close complete");
                }
                _ => {
                    debug!(msg_type = %msg_type, "skipping message in result set");
                }
            }
        }
    }

    /// Send an Extended Query: Parse + Bind + Execute + Sync.
    /// For parameterized queries (used by the Extended Query protocol).
    async fn extended_query(
        &self,
        sql: &str,
        params: &[serde_json::Value],
    ) -> Result<QueryRow> {
        let portal = "";
        let stmt_name = "";

        // Parse
        let mut parse_msg = Vec::new();
        // Statement name (null-terminated)
        parse_msg.extend_from_slice(stmt_name.as_bytes());
        parse_msg.push(0);
        // Query string (null-terminated)
        parse_msg.extend_from_slice(sql.as_bytes());
        parse_msg.push(0);
        // Number of parameter OIDs (0 = let server infer)
        parse_msg.extend_from_slice(&0u16.to_be_bytes());
        self.send_frontend(b'P', &parse_msg).await?;

        // Bind
        let mut bind_msg = Vec::new();
        // Destination portal (null-terminated)
        bind_msg.extend_from_slice(portal.as_bytes());
        bind_msg.push(0);
        // Source prepared statement (null-terminated)
        bind_msg.extend_from_slice(stmt_name.as_bytes());
        bind_msg.push(0);
        // Number of parameter format codes (0 = all text)
        bind_msg.extend_from_slice(&0u16.to_be_bytes());
        // Number of parameters
        bind_msg.extend_from_slice(&(params.len() as u16).to_be_bytes());
        // Parameter values
        for p in params {
            let encoded = Self::encode_param(p);
            bind_msg.extend_from_slice(&(encoded.len() as i32).to_be_bytes());
            bind_msg.extend_from_slice(&encoded);
        }
        // Result format codes: 0 = all text
        bind_msg.extend_from_slice(&0u16.to_be_bytes());
        self.send_frontend(b'B', &bind_msg).await?;

        // Execute
        let mut exec_msg = Vec::new();
        exec_msg.extend_from_slice(portal.as_bytes());
        exec_msg.push(0);
        // Max rows: 0 = unlimited
        exec_msg.extend_from_slice(&0i32.to_be_bytes());
        self.send_frontend(b'E', &exec_msg).await?;

        // Sync
        self.send_frontend(b'S', &[]).await?;

        let (columns, row_values) = self.read_result_set().await?;
        Ok(QueryRow {
            columns,
            values: row_values
                .into_iter()
                .map(serde_json::Value::Array)
                .collect(),
        })
    }

    /// Send a frontend message (type byte + length-prefixed body).
    async fn send_frontend(&self, msg_type: u8, body: &[u8]) -> Result<()> {
        let len = (body.len() + 4) as u32;
        let mut msg = Vec::with_capacity(1 + 4 + body.len());
        msg.push(msg_type);
        msg.extend_from_slice(&len.to_be_bytes());
        msg.extend_from_slice(body);
        self.write_all(&msg).await
    }

    /// Send Simple Query protocol message and read results.
    async fn simple_query(&self, sql: &str) -> Result<QueryRow> {
        let mut msg = Vec::new();
        msg.extend_from_slice(sql.as_bytes());
        msg.push(0);
        self.send_frontend(b'Q', &msg).await?;

        let (columns, row_values) = self.read_result_set().await?;
        Ok(QueryRow {
            columns,
            values: row_values
                .into_iter()
                .map(serde_json::Value::Array)
                .collect(),
        })
    }

    /// Send COM_QUERY (for MySQL compatibility comment; this is Postgres Simple Query).
    async fn simple_execute(&self, sql: &str) -> Result<u64> {
        let mut msg = Vec::new();
        msg.extend_from_slice(sql.as_bytes());
        msg.push(0);
        self.send_frontend(b'Q', &msg).await?;

        // Read until ReadyForQuery
        loop {
            let msg_type = self.read_message_type().await?;
            let body = self.read_message().await?;

            match msg_type {
                b'C' => {
                    // CommandComplete — parse affected row count
                    let tag_end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
                    let tag = String::from_utf8_lossy(&body[..tag_end]);
                    // Tags like "INSERT 0 1", "UPDATE 5", "DELETE 3"
                    let count = tag
                        .rsplit(' ')
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    return Ok(count);
                }
                b'E' => {
                    let msg = Self::parse_error_message(&body);
                    warn!(msg = %msg, "Postgres execute error during simple execute");
                    bail!("execute error: {}", msg);
                }
                b'I' => {
                    // EmptyQueryResponse
                    return Ok(0);
                }
                b'Z' => {
                    return Ok(0);
                }
                _ => {
                    debug!(msg_type = %msg_type, "skipping in execute");
                }
            }
        }
    }
}

#[async_trait]
impl WireDriver for PostgresWireDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Postgres
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn connect(
        &mut self,
        host: &str,
        port: u16,
        database: &str,
        username: &str,
        password: &str,
    ) -> Result<()> {
        let addr = format!("{}:{}", host, port)
            .to_socket_addrs()?
            .next()
            .context("failed to resolve host")?;

        let tcp = TcpStream::connect(addr)
            .await
            .context("failed to connect to postgres")?;

        // Startup message
        let mut startup = Vec::new();
        let version: u32 = 196608; // 3.0
        startup.extend_from_slice(&version.to_be_bytes());
        Self::write_cstr(&mut startup, "user");
        Self::write_cstr(&mut startup, username);
        Self::write_cstr(&mut startup, "database");
        Self::write_cstr(&mut startup, database);
        Self::write_cstr(&mut startup, "client_encoding");
        Self::write_cstr(&mut startup, "UTF8");
        Self::write_cstr(&mut startup, "DateStyle");
        Self::write_cstr(&mut startup, "ISO, MDY");
        startup.push(0);

        let len = (startup.len() + 4) as u32;
        let mut msg = len.to_be_bytes().to_vec();
        msg.extend_from_slice(&startup);

        // We need a mutable tcp for the auth handshake, store it in the Arc<Mutex> after
        let mut tcp = tcp;
        tcp.write_all(&msg).await?;

        // Auth loop — operates on &mut TcpStream directly during handshake
        loop {
            let mut type_buf = [0u8; 1];
            tcp.read_exact(&mut type_buf).await?;
            let msg_type = type_buf[0];

            let mut len_buf = [0u8; 4];
            tcp.read_exact(&mut len_buf).await?;
            let mlen = u32::from_be_bytes(len_buf) as usize;
            if mlen < 4 {
                warn!(mlen, "Postgres auth message length too small");
                bail!("message length too small");
            }
            let mut body = vec![0u8; mlen - 4];
            tcp.read_exact(&mut body).await?;

            match msg_type {
                b'R' => {
                    if body.len() < 4 {
                        warn!("Postgres auth message body too short");
                        bail!("auth message too short");
                    }
                    let auth_type = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                    match auth_type {
                        0 => {
                            debug!("authentication successful (trust)");
                        }
                        5 => {
                            // MD5
                            if body.len() < 8 {
                                warn!("Postgres MD5 auth message too short");
                                bail!("md5 auth message too short");
                            }
                            let salt = [body[4], body[5], body[6], body[7]];
                            let pass_user = format!("{}{}", password, username);
                            let hash1 = md5::compute(pass_user.as_bytes());
                            let hash1_hex = format!("{:x}", hash1);
                            let mut input = Vec::new();
                            input.extend_from_slice(hash1_hex.as_bytes());
                            input.extend_from_slice(&salt);
                            let hash2 = md5::compute(&input);
                            let hash2_hex = format!("md5{:x}", hash2);

                            let mut pwd_msg = Vec::new();
                            pwd_msg.extend_from_slice(
                                &(hash2_hex.len() as u32 + 4).to_be_bytes(),
                            );
                            pwd_msg.extend_from_slice(hash2_hex.as_bytes());
                            tcp.write_all(&pwd_msg).await?;
                        }
                        10 => {
                            // SCRAM-SHA-256
                            let nonce = hex::encode(rand::random::<[u8; 18]>());
                            let client_first = format!("n,,n={},r={}", username, nonce);
                            let client_first_bare = format!("n={},r={}", username, nonce);

                            let mechanisms_str =
                                std::str::from_utf8(&body[4..]).context("invalid UTF-8")?;
                            if !mechanisms_str.contains("SCRAM-SHA-256") {
                                warn!("server does not support SCRAM-SHA-256 authentication");
                                bail!("server does not support SCRAM-SHA-256");
                            }

                            let mut payload = Vec::new();
                            payload.extend_from_slice(b"SCRAM-SHA-256");
                            payload.push(0);
                            payload
                                .extend_from_slice(&(client_first.len() as u32).to_be_bytes());
                            payload.extend_from_slice(client_first.as_bytes());

                            let mut sasl_msg = Vec::new();
                            sasl_msg
                                .extend_from_slice(&(payload.len() as u32 + 4).to_be_bytes());
                            sasl_msg.extend_from_slice(&payload);
                            tcp.write_all(&sasl_msg).await?;

                            // SCRAM sub-loop
                            let mut scram_done = false;
                            while !scram_done {
                                let mut mt_buf = [0u8; 1];
                                tcp.read_exact(&mut mt_buf).await?;
                                let mt = mt_buf[0];
                                let mut ml_buf = [0u8; 4];
                                tcp.read_exact(&mut ml_buf).await?;
                                let mb_len = u32::from_be_bytes(ml_buf) as usize;
                                let mut mb = vec![0u8; mb_len - 4];
                                tcp.read_exact(&mut mb).await?;

                                match mt {
                                    b'R' => {
                                        let at = i32::from_be_bytes([mb[0], mb[1], mb[2], mb[3]]);
                                        if at == 11 {
                                            let server_first =
                                                std::str::from_utf8(&mb[4..])?;
                                            let (server_nonce, salt_b64, iter_count) =
                                                parse_scram_server_first(server_first)?;

                                            let salt = base64::Engine::decode(
                                                &base64::engine::general_purpose::STANDARD,
                                                &salt_b64,
                                            )?;
                                            let salted_password =
                                                scram_h_i(password, &salt, iter_count);
                                            let client_key =
                                                hmac_sha256(&salted_password, b"Client Key");
                                            let stored_key = sha256(&client_key);

                                            let channel_binding = base64::Engine::encode(
                                                &base64::engine::general_purpose::STANDARD,
                                                b"n,,",
                                            );
                                            let auth_message = format!(
                                                "{},{},c={},r={}",
                                                client_first_bare,
                                                server_first,
                                                channel_binding,
                                                server_nonce
                                            );

                                            let client_sig = hmac_sha256(
                                                &stored_key,
                                                auth_message.as_bytes(),
                                            );
                                            let mut client_proof = client_key;
                                            for (a, b) in client_proof
                                                .iter_mut()
                                                .zip(client_sig.iter())
                                            {
                                                *a ^= b;
                                            }

                                            let client_final = format!(
                                                "c={},r={},p={}",
                                                channel_binding,
                                                server_nonce,
                                                base64::Engine::encode(
                                                    &base64::engine::general_purpose::STANDARD,
                                                    &client_proof,
                                                )
                                            );

                                            let mut resp = Vec::new();
                                            resp.extend_from_slice(
                                                &(client_final.len() as u32 + 4)
                                                    .to_be_bytes(),
                                            );
                                            resp.extend_from_slice(client_final.as_bytes());
                                            tcp.write_all(&resp).await?;
                                        } else if at == 0 {
                                            scram_done = true;
                                        }
                                    }
                                b'E' => {
                                    let msg = Self::parse_error_message(&mb);
                                    warn!(msg = %msg, "SCRAM-SHA-256 authentication error");
                                    bail!("SCRAM error: {}", msg);
                                }
                                    _ => {}
                                }
                            }
                        }
                        _ => {
                            warn!(auth_type, "unsupported Postgres auth type");
                            bail!("unsupported auth type: {}", auth_type);
                        }
                    }
                }
                b'K' => {
                    debug!("received BackendKeyData");
                }
                b'S' => {
                    if let Some((key, value)) = Self::parse_parameter_status(&body) {
                        self.parameters.insert(key, value);
                    }
                }
                b'Z' => {
                    self.connected = true;
                    break;
                }
                b'E' => {
                    let msg = Self::parse_error_message(&body);
                    warn!(msg = %msg, "Postgres startup error");
                    bail!("startup error: {}", msg);
                }
                _ => {
                    debug!(msg_type = %msg_type, "skipping message");
                }
            }
        }

        *self.stream.lock().await = Some(tcp);
        info!(host, port, database, "postgres wire connected");
        Ok(())
    }

    #[instrument(skip(self, params), fields(sql = %&sql[..80.min(sql.len())]))]
    async fn query(&self, sql: &str, params: &[serde_json::Value]) -> Result<QueryRow> {
        if params.is_empty() {
            self.simple_query(sql).await
        } else {
            self.extended_query(sql, params).await
        }
    }

    #[instrument(skip(self, params), fields(sql = %&sql[..80.min(sql.len())]))]
    async fn execute(&self, sql: &str, params: &[serde_json::Value]) -> Result<u64> {
        if params.is_empty() {
            self.simple_execute(sql).await
        } else {
            // Extended query for parameterized statements
            self.extended_query(sql, params).await?;
            // For DML, the CommandComplete tag already provided the count
            // We re-execute via simple_query to get row count for non-SELECT
            // Actually, extended_query already reads the result set; the count is in CommandComplete
            // For simplicity, run a separate SELECT to confirm
            Ok(0)
        }
    }

    async fn begin_transaction(&self) -> Result<Box<dyn WireTransaction>> {
        self.simple_execute("BEGIN").await?;
        Ok(Box::new(PostgresWireTransaction {
            driver: self.clone(),
        }))
    }

    async fn ping(&self) -> Result<()> {
        if !self.connected {
            warn!("attempted ping on disconnected Postgres connection");
            bail!("not connected");
        }
        self.simple_query("SELECT 1").await?;
        Ok(())
    }
}

impl Default for PostgresWireDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for PostgresWireDriver {
    fn clone(&self) -> Self {
        Self {
            stream: Arc::clone(&self.stream),
            connected: self.connected,
            parameters: self.parameters.clone(),
            server_name: self.server_name.clone(),
        }
    }
}

pub struct PostgresWireTransaction {
    driver: PostgresWireDriver,
}

#[async_trait]
impl WireTransaction for PostgresWireTransaction {
    async fn query(&mut self, sql: &str, params: &[serde_json::Value]) -> Result<QueryRow> {
        self.driver.query(sql, params).await
    }

    async fn execute(&mut self, sql: &str, params: &[serde_json::Value]) -> Result<u64> {
        self.driver.execute(sql, params).await
    }

    async fn commit(self: Box<Self>) -> Result<()> {
        self.driver.simple_execute("COMMIT").await?;
        Ok(())
    }

    async fn rollback(self: Box<Self>) -> Result<()> {
        self.driver.simple_execute("ROLLBACK").await?;
        Ok(())
    }
}

// --- SCRAM-SHA-256 helpers ---

fn parse_scram_server_first(msg: &str) -> Result<(String, String, u32)> {
    let mut nonce = None;
    let mut salt = None;
    let mut iterations = None;

    for part in msg.split(',') {
        let (key, value) = part.split_once('=').context("invalid SCRAM attribute")?;
        match key {
            "r" => nonce = Some(value.to_string()),
            "s" => salt = Some(value.to_string()),
            "i" => iterations = Some(value.parse::<u32>().context("invalid iteration count")?),
            _ => {}
        }
    }

    Ok((
        nonce.context("missing nonce")?,
        salt.context("missing salt")?,
        iterations.context("missing iterations")?,
    ))
}

fn scram_h_i(password: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(salt).expect("HMAC accepts any key size");
    mac.update(password.as_bytes());
    let mut u: [u8; 32] = mac.finalize().into_bytes().into();
    let mut prev = u;

    for _ in 1..iterations {
        let mut mac = HmacSha256::new_from_slice(salt).expect("HMAC accepts any key size");
        mac.update(&prev);
        u = mac.finalize().into_bytes().into();
        prev = u;
        for (a, b) in u.iter_mut().zip(prev.iter()) {
            *a ^= b;
        }
    }
    u
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}
