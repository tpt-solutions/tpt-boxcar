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

    fn next_seq(&self) -> u8 {
        self.sequence_id.fetch_add(1, Ordering::SeqCst)
    }

    fn current_seq(&self) -> u8 {
        self.sequence_id.load(Ordering::SeqCst)
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

    fn native_password_hash(password: &str) -> Vec<u8> {
        use sha1::{Digest, Sha1};
        let mut sha1_pass = Sha1::new();
        sha1_pass.update(password.as_bytes());
        let stage1: [u8; 20] = sha1_pass.finalize().into();

        let mut sha1_stage1 = Sha1::new();
        sha1_stage1.update(&stage1);
        let stage2: [u8; 20] = sha1_stage1.finalize().into();

        let mut sha1_full = Sha1::new();
        sha1_full.update(password.as_bytes());
        sha1_full.update(&stage2);
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
        let cap_offset = 1 + version_end + 1 + 4 + 8 + 1;
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
            let hash = Self::native_password_hash(password);
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
                let new_plugin = if let Some(end) =
                    response[switch_pos..].iter().position(|&b| b == 0)
                {
                    std::str::from_utf8(&response[switch_pos..switch_pos + end])
                        .context("invalid auth plugin name")?
                        .to_string()
                } else {
                    warn!("malformed auth switch request from server");
                    bail!("malformed auth switch request");
                };

                debug!(plugin = %new_plugin, "auth switch requested");

                if new_plugin == "mysql_native_password" && !password.is_empty() {
                    let hash = Self::native_password_hash(password);
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
        let seq = self.next_seq();

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

            // Read param definitions until EOF
            loop {
                let pkt = Self::read_packet(stream).await?;
                if pkt.is_empty() || pkt[0] == 0xFE {
                    break;
                }
            }

            // COM_STMT_EXECUTE
            let exec_seq = self.next_seq();
            let mut exec_payload = Vec::new();
            exec_payload.push(0x17);
            exec_payload.extend_from_slice(&stmt_id.to_le_bytes());
            exec_payload.push(0x00);
            exec_payload.extend_from_slice(&1u32.to_le_bytes());

            let null_bitmap_len = (params.len() + 7) / 8;
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
                        exec_payload.extend_from_slice(&(s.len() as u32).to_le_bytes());
                        exec_payload.extend_from_slice(s.as_bytes());
                    }
                    _ => {
                        let s = p.to_string();
                        exec_payload.extend_from_slice(&(s.len() as u32).to_le_bytes());
                        exec_payload.extend_from_slice(s.as_bytes());
                    }
                }
            }

            Self::write_packet(stream, exec_seq, &exec_payload).await?;
            let result = Self::com_query(stream, "SELECT 1", self.next_seq()).await?;
            return Ok(result);
        }

        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected")?;
        Self::com_query(stream, sql, seq).await
    }

    #[instrument(skip(self, _params), fields(sql = %&sql[..80.min(sql.len())]))]
    pub async fn execute(&self, sql: &str, _params: &[serde_json::Value]) -> Result<u64> {
        let seq = self.next_seq();
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected")?;
        let result = Self::com_query(stream, sql, seq).await?;
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
        let seq = self.next_seq();
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