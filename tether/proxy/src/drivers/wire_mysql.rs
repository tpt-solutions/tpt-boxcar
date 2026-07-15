use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, info, instrument, warn};

use super::wire::DriverKind;
use super::QueryRow;

const CLIENT_PROTOCOL_41: u32 = 1 << 9;
const CLIENT_SECURE_CONNECTION: u32 = 1 << 15;
const CLIENT_PLUGIN_AUTH: u32 = 1 << 19;
const CLIENT_CONNECT_WITH_DB: u32 = 1 << 3;
const CLIENT_MULTI_STATEMENTS: u32 = 1 << 16;

pub struct MysqlWireDriver {
    stream: Arc<Mutex<Option<TcpStream>>>,
    connected: bool,
    sequence_id: AtomicU8,
}

impl std::fmt::Debug for MysqlWireDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlWireDriver")
            .field("connected", &self.connected)
            .finish()
    }
}

impl MysqlWireDriver {
    pub fn new() -> Self {
        Self {
            stream: Arc::new(Mutex::new(None)),
            connected: false,
            sequence_id: AtomicU8::new(0),
        }
    }

    /// Reset the packet sequence for a new top-level command. Every MySQL
    /// command (COM_QUERY, COM_STMT_PREPARE, COM_STMT_EXECUTE, COM_PING, ...)
    /// must start its own packet sequence at 0 — it does not continue from
    /// whatever the previous command left off.
    fn start_command(&self) -> u8 {
        self.sequence_id.store(1, Ordering::SeqCst);
        0
    }

    async fn read_packet(stream: &mut TcpStream) -> Result<Vec<u8>> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await?;
        let len = u32::from_le_bytes([header[0], header[1], header[2], 0]) as usize;
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await?;
        Ok(payload)
    }

    async fn write_packet(stream: &mut TcpStream, seq_id: u8, payload: &[u8]) -> Result<()> {
        let len = payload.len() as u32;
        let mut header = [0u8; 4];
        header[0] = (len & 0xFF) as u8;
        header[1] = ((len >> 8) & 0xFF) as u8;
        header[2] = ((len >> 16) & 0xFF) as u8;
        header[3] = seq_id;
        stream.write_all(&header).await?;
        stream.write_all(payload).await?;
        Ok(())
    }

    /// mysql_native_password token: SHA1(password) XOR SHA1(scramble + SHA1(SHA1(password))).
    /// The scramble is the server-issued random nonce from the handshake (or
    /// auth-switch request) — omitting it would produce a static, replayable
    /// hash that no real server would ever accept.
    fn native_password_hash(password: &str, scramble: &[u8]) -> Vec<u8> {
        use sha1::{Digest, Sha1};
        let mut sha1_pass = Sha1::new();
        sha1_pass.update(password.as_bytes());
        let stage1: [u8; 20] = sha1_pass.finalize().into();

        let mut sha1_stage1 = Sha1::new();
        sha1_stage1.update(stage1);
        let stage2: [u8; 20] = sha1_stage1.finalize().into();

        let mut sha1_full = Sha1::new();
        sha1_full.update(scramble);
        sha1_full.update(stage2);
        let stage3: [u8; 20] = sha1_full.finalize().into();

        let mut result = [0u8; 20];
        for i in 0..20 {
            result[i] = stage1[i] ^ stage3[i];
        }
        result.to_vec()
    }

    async fn handshake(
        stream: &mut TcpStream,
        username: &str,
        password: &str,
        database: &str,
    ) -> Result<()> {
        let handshake = Self::read_packet(stream).await?;
        if !handshake.is_empty() && handshake[0] == 0xFF {
            warn!("server rejected connection during handshake");
            bail!("server rejected connection during handshake");
        }

        let version_end = handshake[1..]
            .iter()
            .position(|&b| b == 0)
            .context("missing null terminator in server version")?;
        // auth-plugin-data-part-1 (first 8 bytes of the scramble) follows the
        // connection id, which follows the null-terminated version string.
        let salt1_offset = 1 + version_end + 1 + 4;
        let cap_offset = salt1_offset + 8 + 1;
        // auth-plugin-data-part-2 (remaining ~12 scramble bytes) follows
        // capability_flags_lower(2) + charset(1) + status(2) + capability_flags_upper(2)
        // + auth_plugin_data_len(1) + 10 reserved bytes.
        let salt2_offset = cap_offset + 2 + 1 + 2 + 2 + 1 + 10;
        let mut scramble = Vec::with_capacity(20);
        if handshake.len() >= salt1_offset + 8 {
            scramble.extend_from_slice(&handshake[salt1_offset..salt1_offset + 8]);
        }
        if handshake.len() >= salt2_offset + 12 {
            scramble.extend_from_slice(&handshake[salt2_offset..salt2_offset + 12]);
        }

        let server_capabilities: u32 = if handshake.len() >= cap_offset + 4 {
            u32::from_le_bytes([
                handshake[cap_offset],
                handshake[cap_offset + 1],
                handshake[cap_offset + 2],
                handshake[cap_offset + 3],
            ])
        } else {
            0
        };

        let mut capabilities: u32 =
            CLIENT_PROTOCOL_41 | CLIENT_SECURE_CONNECTION | CLIENT_PLUGIN_AUTH;
        if !database.is_empty() && (server_capabilities & CLIENT_CONNECT_WITH_DB) != 0 {
            capabilities |= CLIENT_CONNECT_WITH_DB;
        }
        if (server_capabilities & CLIENT_MULTI_STATEMENTS) != 0 {
            capabilities |= CLIENT_MULTI_STATEMENTS;
        }

        let mut resp = Vec::new();
        resp.extend_from_slice(&capabilities.to_le_bytes());
        resp.extend_from_slice(&16777216u32.to_le_bytes());
        resp.push(45); // utf8mb4
        resp.extend_from_slice(&[0u8; 23]);
        resp.extend_from_slice(username.as_bytes());
        resp.push(0);

        if !password.is_empty() {
            let hash = Self::native_password_hash(password, &scramble);
            resp.push(hash.len() as u8);
            resp.extend_from_slice(&hash);
        } else {
            resp.push(0);
        }

        if !database.is_empty() {
            resp.extend_from_slice(database.as_bytes());
            resp.push(0);
        }

        resp.extend_from_slice(b"mysql_native_password");
        resp.push(0);

        Self::write_packet(stream, 1, &resp).await?;

        let response = Self::read_packet(stream).await?;
        if response.is_empty() {
            warn!("empty handshake response from server");
            bail!("empty handshake response");
        }
        match response[0] {
            0x00 => {
                debug!("MySQL handshake successful");
                Ok(())
            }
            0xFF => {
                let err_code = if response.len() >= 3 {
                    u16::from_le_bytes([response[1], response[2]])
                } else {
                    0
                };
                let err_msg = if response.len() > 3 {
                    String::from_utf8_lossy(&response[3..]).to_string()
                } else {
                    "unknown error".to_string()
                };
                warn!(err_code, err_msg = %err_msg, "MySQL auth error during handshake");
                bail!("MySQL auth error {}: {}", err_code, err_msg);
            }
            0xFE => {
                let switch_pos = 1;
                let (new_plugin, name_end) =
                    if let Some(end) = response[switch_pos..].iter().position(|&b| b == 0) {
                        (
                            std::str::from_utf8(&response[switch_pos..switch_pos + end])
                                .context("invalid auth plugin name")?
                                .to_string(),
                            end,
                        )
                    } else {
                        warn!("malformed auth switch request from server");
                        bail!("malformed auth switch request");
                    };

                debug!(plugin = %new_plugin, "auth switch requested");

                // New scramble bytes follow the null-terminated plugin name.
                let new_scramble_start = switch_pos + name_end + 1;
                let new_scramble: Vec<u8> = response
                    .get(new_scramble_start..)
                    .map(|s| s.iter().take_while(|&&b| b != 0).copied().collect())
                    .unwrap_or_default();

                if new_plugin == "mysql_native_password" && !password.is_empty() {
                    let hash = Self::native_password_hash(password, &new_scramble);
                    Self::write_packet(stream, 2, &hash).await?;

                    let auth_response = Self::read_packet(stream).await?;
                    if !auth_response.is_empty() && auth_response[0] == 0x00 {
                        debug!("MySQL auth switch successful");
                        Ok(())
                    } else {
                        warn!("MySQL auth switch failed after plugin response");
                        bail!("MySQL auth switch failed");
                    }
                } else {
                    warn!(plugin = %new_plugin, "unsupported auth plugin requested by server");
                    bail!("unsupported auth plugin: {}", new_plugin);
                }
            }
            _ => {
                debug!(
                    "MySQL handshake completed with status byte: 0x{:02X}",
                    response[0]
                );
                Ok(())
            }
        }
    }

    async fn com_query(stream: &mut TcpStream, sql: &str, seq: u8) -> Result<QueryRow> {
        let mut payload = Vec::new();
        payload.push(0x03); // COM_QUERY
        payload.extend_from_slice(sql.as_bytes());
        Self::write_packet(stream, seq, &payload).await?;

        let mut columns = Vec::new();
        let rows: Vec<Vec<serde_json::Value>> = Vec::new();

        loop {
            let packet = Self::read_packet(stream).await?;
            if packet.is_empty() {
                warn!("empty packet received during query execution");
                bail!("empty packet");
            }

            match packet[0] {
                0x00 => {
                    let affected = if packet.len() >= 7 {
                        let mut pos = 1;
                        read_lenenc_int(&packet, &mut pos)
                    } else {
                        0
                    };
                    return Ok(QueryRow {
                        columns: vec!["affected_rows".to_string()],
                        values: vec![serde_json::json!(affected)],
                    });
                }
                0xFF => {
                    let err_code = if packet.len() >= 3 {
                        u16::from_le_bytes([packet[1], packet[2]])
                    } else {
                        0
                    };
                    let err_msg = if packet.len() > 3 {
                        String::from_utf8_lossy(&packet[3..]).to_string()
                    } else {
                        "unknown error".to_string()
                    };
                    warn!(err_code, err_msg = %err_msg, "MySQL error during query execution");
                    bail!("MySQL error {}: {}", err_code, err_msg);
                }
                0xFE => {
                    if !columns.is_empty() && rows.is_empty() {
                        continue;
                    }
                    return Ok(QueryRow {
                        columns,
                        values: rows.into_iter().map(serde_json::Value::Array).collect(),
                    });
                }
                _ => {
                    let mut pos = 0;
                    let _catalog = read_lenenc_str(&packet, &mut pos);
                    let _schema = read_lenenc_str(&packet, &mut pos);
                    let _table = read_lenenc_str(&packet, &mut pos);
                    let _org_table = read_lenenc_str(&packet, &mut pos);
                    let name = read_lenenc_str(&packet, &mut pos);
                    columns.push(name);
                }
            }
        }
    }

    /// Parse a column-definition packet, returning (name, MySQL column type byte).
    fn parse_column_def(packet: &[u8]) -> (String, u8) {
        let mut pos = 0;
        let _catalog = read_lenenc_str(packet, &mut pos);
        let _schema = read_lenenc_str(packet, &mut pos);
        let _table = read_lenenc_str(packet, &mut pos);
        let _org_table = read_lenenc_str(packet, &mut pos);
        let name = read_lenenc_str(packet, &mut pos);
        let _org_name = read_lenenc_str(packet, &mut pos);
        let _fixed_len = read_lenenc_int(packet, &mut pos); // always 0x0c
        pos += 2; // character set
        pos += 4; // column length
        let col_type = packet.get(pos).copied().unwrap_or(0);
        (name, col_type)
    }

    /// Decode a single binary-protocol column value at `pos`, advancing `pos` past it.
    fn decode_binary_value(pkt: &[u8], pos: &mut usize, col_type: u8) -> Result<serde_json::Value> {
        Ok(match col_type {
            0x01 => {
                // TINY
                let v = *pkt.get(*pos).context("truncated tiny value")? as i8;
                *pos += 1;
                serde_json::json!(v)
            }
            0x02 => {
                // SHORT
                let v = i16::from_le_bytes([
                    *pkt.get(*pos).context("truncated short value")?,
                    *pkt.get(*pos + 1).context("truncated short value")?,
                ]);
                *pos += 2;
                serde_json::json!(v)
            }
            0x03 | 0x09 => {
                // LONG, INT24
                let v = i32::from_le_bytes([
                    *pkt.get(*pos).context("truncated long value")?,
                    *pkt.get(*pos + 1).context("truncated long value")?,
                    *pkt.get(*pos + 2).context("truncated long value")?,
                    *pkt.get(*pos + 3).context("truncated long value")?,
                ]);
                *pos += 4;
                serde_json::json!(v)
            }
            0x08 => {
                // LONGLONG
                let v = i64::from_le_bytes([
                    *pkt.get(*pos).context("truncated longlong value")?,
                    *pkt.get(*pos + 1).context("truncated longlong value")?,
                    *pkt.get(*pos + 2).context("truncated longlong value")?,
                    *pkt.get(*pos + 3).context("truncated longlong value")?,
                    *pkt.get(*pos + 4).context("truncated longlong value")?,
                    *pkt.get(*pos + 5).context("truncated longlong value")?,
                    *pkt.get(*pos + 6).context("truncated longlong value")?,
                    *pkt.get(*pos + 7).context("truncated longlong value")?,
                ]);
                *pos += 8;
                serde_json::json!(v)
            }
            0x04 => {
                // FLOAT
                let v = f32::from_le_bytes([
                    *pkt.get(*pos).context("truncated float value")?,
                    *pkt.get(*pos + 1).context("truncated float value")?,
                    *pkt.get(*pos + 2).context("truncated float value")?,
                    *pkt.get(*pos + 3).context("truncated float value")?,
                ]);
                *pos += 4;
                serde_json::json!(v)
            }
            0x05 => {
                // DOUBLE
                let v = f64::from_le_bytes([
                    *pkt.get(*pos).context("truncated double value")?,
                    *pkt.get(*pos + 1).context("truncated double value")?,
                    *pkt.get(*pos + 2).context("truncated double value")?,
                    *pkt.get(*pos + 3).context("truncated double value")?,
                    *pkt.get(*pos + 4).context("truncated double value")?,
                    *pkt.get(*pos + 5).context("truncated double value")?,
                    *pkt.get(*pos + 6).context("truncated double value")?,
                    *pkt.get(*pos + 7).context("truncated double value")?,
                ]);
                *pos += 8;
                serde_json::json!(v)
            }
            // VARCHAR, VAR_STRING, STRING, BLOB, NEWDECIMAL, date/time types, etc.
            _ => serde_json::Value::String(read_lenenc_str(pkt, pos)),
        })
    }

    /// Read the response to COM_STMT_EXECUTE: either an OK packet (DML, no
    /// result set) or a binary-protocol result set.
    async fn read_binary_resultset(stream: &mut TcpStream) -> Result<QueryRow> {
        let first = Self::read_packet(stream).await?;
        if first.is_empty() {
            warn!("empty packet received reading execute response");
            bail!("empty packet");
        }

        if first[0] == 0x00 {
            let mut pos = 1;
            let affected = read_lenenc_int(&first, &mut pos);
            return Ok(QueryRow {
                columns: vec!["affected_rows".to_string()],
                values: vec![serde_json::json!(affected)],
            });
        }

        if first[0] == 0xFF {
            let err_code = if first.len() >= 3 {
                u16::from_le_bytes([first[1], first[2]])
            } else {
                0
            };
            let err_msg = if first.len() > 3 {
                String::from_utf8_lossy(&first[3..]).to_string()
            } else {
                "unknown error".to_string()
            };
            warn!(err_code, err_msg = %err_msg, "MySQL error during execute");
            bail!("MySQL error {}: {}", err_code, err_msg);
        }

        let mut pos = 0;
        let column_count = read_lenenc_int(&first, &mut pos) as usize;

        let mut columns = Vec::with_capacity(column_count);
        let mut col_types = Vec::with_capacity(column_count);
        for _ in 0..column_count {
            let pkt = Self::read_packet(stream).await?;
            let (name, col_type) = Self::parse_column_def(&pkt);
            columns.push(name);
            col_types.push(col_type);
        }
        if column_count > 0 {
            let eof = Self::read_packet(stream).await?;
            if eof.is_empty() || eof[0] != 0xFE {
                warn!("expected EOF after column definitions");
                bail!("expected EOF after column definitions");
            }
        }

        let null_bitmap_len = (column_count + 7 + 2) / 8;
        let mut rows = Vec::new();
        loop {
            let pkt = Self::read_packet(stream).await?;
            if pkt.is_empty() {
                warn!("empty packet received while reading binary row");
                bail!("empty packet");
            }
            if pkt[0] == 0xFE && pkt.len() < 9 {
                break;
            }
            if pkt[0] == 0xFF {
                let err_code = u16::from_le_bytes([pkt[1], pkt[2]]);
                let err_msg = String::from_utf8_lossy(&pkt[3..]).to_string();
                warn!(err_code, err_msg = %err_msg, "MySQL error during binary row read");
                bail!("MySQL error {}: {}", err_code, err_msg);
            }

            let null_bitmap = &pkt[1..1 + null_bitmap_len];
            let mut vpos = 1 + null_bitmap_len;
            let mut row_values = Vec::with_capacity(column_count);
            for (i, &col_type) in col_types.iter().enumerate() {
                let byte_idx = (i + 2) / 8;
                let bit_idx = (i + 2) % 8;
                let is_null = null_bitmap
                    .get(byte_idx)
                    .map(|b| (b >> bit_idx) & 1 == 1)
                    .unwrap_or(false);
                if is_null {
                    row_values.push(serde_json::Value::Null);
                    continue;
                }
                row_values.push(Self::decode_binary_value(&pkt, &mut vpos, col_type)?);
            }
            rows.push(serde_json::Value::Array(row_values));
        }

        Ok(QueryRow {
            columns,
            values: rows,
        })
    }
}

impl MysqlWireDriver {
    pub fn kind(&self) -> DriverKind {
        DriverKind::Mysql
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    #[instrument(skip(self, params), fields(sql = %&sql[..80.min(sql.len())]))]
    pub async fn query(&self, sql: &str, params: &[serde_json::Value]) -> Result<QueryRow> {
        let seq = self.start_command();

        if !params.is_empty() {
            let mut guard = self.stream.lock().await;
            let stream = guard.as_mut().context("not connected")?;

            // COM_STMT_PREPARE
            let mut parse_payload = Vec::new();
            parse_payload.push(0x16);
            parse_payload.extend_from_slice(sql.as_bytes());
            Self::write_packet(stream, seq, &parse_payload).await?;

            let prepare_response = Self::read_packet(stream).await?;
            if prepare_response.is_empty() || prepare_response[0] == 0xFF {
                warn!("COM_STMT_PREPARE failed for query");
                bail!("prepare failed");
            }
            let stmt_id = u32::from_le_bytes([
                prepare_response[1],
                prepare_response[2],
                prepare_response[3],
                prepare_response[4],
            ]);
            let num_columns = u16::from_le_bytes([prepare_response[5], prepare_response[6]]);
            let num_params = u16::from_le_bytes([prepare_response[7], prepare_response[8]]);

            // Read and discard parameter-definition packets + trailing EOF
            if num_params > 0 {
                for _ in 0..num_params {
                    Self::read_packet(stream).await?;
                }
                Self::read_packet(stream).await?;
            }
            // Read and discard column-definition packets + trailing EOF
            // (COM_STMT_EXECUTE resends column definitions, so these aren't needed here)
            if num_columns > 0 {
                for _ in 0..num_columns {
                    Self::read_packet(stream).await?;
                }
                Self::read_packet(stream).await?;
            }

            // COM_STMT_EXECUTE is a new top-level command — its own sequence starts at 0.
            let exec_seq = self.start_command();
            let mut exec_payload = Vec::new();
            exec_payload.push(0x17);
            exec_payload.extend_from_slice(&stmt_id.to_le_bytes());
            exec_payload.push(0x00);
            exec_payload.extend_from_slice(&1u32.to_le_bytes());

            let null_bitmap_len = params.len().div_ceil(8);
            let mut null_bitmap = vec![0u8; null_bitmap_len];
            for (i, p) in params.iter().enumerate() {
                if p.is_null() {
                    null_bitmap[i / 8] |= 1 << (i % 8);
                }
            }
            exec_payload.extend_from_slice(&null_bitmap);
            exec_payload.push(0x01);

            for p in params {
                let type_code = match p {
                    serde_json::Value::Null => 0x06u16,
                    serde_json::Value::Bool(_) => 0x01,
                    serde_json::Value::Number(n) => {
                        if n.is_i64() {
                            0x08
                        } else {
                            0x05
                        }
                    }
                    _ => 0xFD,
                };
                exec_payload.extend_from_slice(&type_code.to_le_bytes());
            }

            for p in params {
                match p {
                    serde_json::Value::Null => {}
                    serde_json::Value::Bool(b) => {
                        exec_payload.push(if *b { 1 } else { 0 });
                    }
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            exec_payload.extend_from_slice(&i.to_le_bytes());
                        } else if let Some(f) = n.as_f64() {
                            exec_payload.extend_from_slice(&f.to_le_bytes());
                        }
                    }
                    serde_json::Value::String(s) => {
                        write_lenenc_int(&mut exec_payload, s.len() as u64);
                        exec_payload.extend_from_slice(s.as_bytes());
                    }
                    _ => {
                        let s = p.to_string();
                        write_lenenc_int(&mut exec_payload, s.len() as u64);
                        exec_payload.extend_from_slice(s.as_bytes());
                    }
                }
            }

            Self::write_packet(stream, exec_seq, &exec_payload).await?;
            let result = Self::read_binary_resultset(stream).await?;
            return Ok(result);
        }

        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected")?;
        Self::com_query(stream, sql, seq).await
    }

    #[instrument(skip(self, params), fields(sql = %&sql[..80.min(sql.len())]))]
    pub async fn execute(&self, sql: &str, params: &[serde_json::Value]) -> Result<u64> {
        let result = self.query(sql, params).await?;
        if let Some(val) = result.values.first() {
            if let Some(v) = val.get("affected_rows").or(Some(val)) {
                return v.as_u64().context("invalid affected rows value");
            }
        }
        Ok(0)
    }

    pub async fn connect(
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

        let mut stream = TcpStream::connect(addr)
            .await
            .context("failed to connect to mysql")?;

        Self::handshake(&mut stream, username, password, database).await?;

        *self.stream.lock().await = Some(stream);
        self.sequence_id.store(0, Ordering::SeqCst);
        self.connected = true;
        info!(host, port, database, "mysql wire connected");
        Ok(())
    }

    pub async fn ping(&self) -> Result<()> {
        if !self.connected {
            warn!("attempted ping on disconnected MySQL connection");
            bail!("not connected");
        }
        let seq = self.start_command();
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected")?;
        Self::write_packet(stream, seq, &[0x0E]).await?; // COM_PING
        let response = Self::read_packet(stream).await?;
        if !response.is_empty() && response[0] == 0x00 {
            Ok(())
        } else {
            warn!("MySQL COM_PING did not return OK");
            bail!("ping failed")
        }
    }
}

impl Default for MysqlWireDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MysqlWireDriver {
    fn clone(&self) -> Self {
        Self {
            stream: Arc::clone(&self.stream),
            connected: self.connected,
            sequence_id: AtomicU8::new(self.sequence_id.load(Ordering::SeqCst)),
        }
    }
}

/// Write a length-encoded integer (used for binary-protocol parameter string
/// lengths). Not a raw fixed-width integer — encoding a length as a plain
/// 4-byte value instead of this format desyncs every byte that follows.
fn write_lenenc_int(buf: &mut Vec<u8>, val: u64) {
    if val < 251 {
        buf.push(val as u8);
    } else if val < 0x10000 {
        buf.push(0xFC);
        buf.extend_from_slice(&(val as u16).to_le_bytes());
    } else if val < 0x1000000 {
        buf.push(0xFD);
        buf.extend_from_slice(&(val as u32).to_le_bytes()[0..3]);
    } else {
        buf.push(0xFE);
        buf.extend_from_slice(&val.to_le_bytes());
    }
}

fn read_lenenc_int(data: &[u8], pos: &mut usize) -> u64 {
    if *pos >= data.len() {
        return 0;
    }
    match data[*pos] {
        0xFB => {
            *pos += 1;
            0
        }
        0xFC => {
            *pos += 1;
            let val = u16::from_le_bytes([
                data.get(*pos).copied().unwrap_or(0),
                data.get(*pos + 1).copied().unwrap_or(0),
            ]);
            *pos += 2;
            val as u64
        }
        0xFD => {
            *pos += 1;
            let val = u32::from_le_bytes([
                data.get(*pos).copied().unwrap_or(0),
                data.get(*pos + 1).copied().unwrap_or(0),
                data.get(*pos + 2).copied().unwrap_or(0),
                0,
            ]);
            *pos += 3;
            val as u64
        }
        0xFE => {
            *pos += 1;
            let val = u64::from_le_bytes([
                data.get(*pos).copied().unwrap_or(0),
                data.get(*pos + 1).copied().unwrap_or(0),
                data.get(*pos + 2).copied().unwrap_or(0),
                data.get(*pos + 3).copied().unwrap_or(0),
                data.get(*pos + 4).copied().unwrap_or(0),
                data.get(*pos + 5).copied().unwrap_or(0),
                data.get(*pos + 6).copied().unwrap_or(0),
                data.get(*pos + 7).copied().unwrap_or(0),
            ]);
            *pos += 8;
            val
        }
        _ => {
            let val = data[*pos] as u64;
            *pos += 1;
            val
        }
    }
}

fn read_lenenc_str(data: &[u8], pos: &mut usize) -> String {
    let len = read_lenenc_int(data, pos) as usize;
    let end = (*pos + len).min(data.len());
    let s = String::from_utf8_lossy(&data[*pos..end]).to_string();
    *pos = end;
    s
}
