use std::net::ToSocketAddrs;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, info, instrument};

use super::wire::DriverKind;
use super::QueryRow;

/// Encodes a Redis command in the RESP "multi bulk" request format.
/// Free function (not tied to `TcpStream`) so it can also be used by the
/// dedicated pub/sub connection in `pubsub.rs`, which needs to write to a
/// split `OwnedWriteHalf` rather than a whole `TcpStream`.
pub(crate) fn encode_resp_command(args: &[&str]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(format!("*{}\r\n", args.len()).as_bytes());
    for arg in args {
        buf.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
        buf.extend_from_slice(arg.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }
    buf
}

/// Reads and decodes one RESP2/RESP3 frame from any `AsyncRead` source.
/// Generic (not tied to `TcpStream`) so it can also be used by the
/// dedicated pub/sub connection in `pubsub.rs`, which reads from a split
/// `OwnedReadHalf`.
pub(crate) async fn read_resp_frame<R>(stream: &mut R) -> Result<RespFrame>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line_buf = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        if byte[0] == b'\n' {
            break;
        }
        if byte[0] != b'\r' {
            line_buf.push(byte[0]);
        }
    }

    let line = String::from_utf8(line_buf).context("invalid RESP frame line")?;
    let type_byte = line.as_bytes()[0];
    let payload = &line[1..];

    match type_byte {
        b'+' => Ok(RespFrame::Simple(payload.to_string())),
        b'-' => Ok(RespFrame::Error(payload.to_string())),
        b':' => {
            let val = payload.parse::<i64>().context("invalid integer")?;
            Ok(RespFrame::Integer(val))
        }
        b'$' => {
            let len: i64 = payload.parse().context("invalid bulk string length")?;
            if len < 0 {
                return Ok(RespFrame::Null);
            }
            let len = len as usize;
            let mut data = vec![0u8; len];
            stream.read_exact(&mut data).await?;
            let mut crlf = [0u8; 2];
            stream.read_exact(&mut crlf).await?;
            Ok(RespFrame::BulkString(data))
        }
        b'*' => {
            let count: i64 = payload.parse().context("invalid array length")?;
            if count < 0 {
                return Ok(RespFrame::Null);
            }
            let mut items = Vec::with_capacity(count as usize);
            for _ in 0..count {
                items.push(Box::pin(read_resp_frame(stream)).await?);
            }
            Ok(RespFrame::Array(items))
        }
        b'_' => Ok(RespFrame::Null),
        b',' => {
            let val = payload.parse::<f64>().context("invalid double")?;
            Ok(RespFrame::Double(val))
        }
        b'(' => {
            let val = payload.parse::<i64>().context("invalid big number")?;
            Ok(RespFrame::BigNumber(val))
        }
        b'=' => {
            let data = payload.as_bytes();
            if data.len() >= 3 {
                let encoding =
                    std::str::from_utf8(&data[..3]).context("invalid verbatim encoding")?;
                let text = std::str::from_utf8(&data[3..]).context("invalid verbatim text")?;
                Ok(RespFrame::VerbatimString(
                    encoding.to_string(),
                    text.to_string(),
                ))
            } else {
                Ok(RespFrame::Simple(payload.to_string()))
            }
        }
        b'#' => {
            let val = match payload {
                "t" => true,
                "f" => false,
                _ => bail!("invalid boolean: {}", payload),
            };
            Ok(RespFrame::Boolean(val))
        }
        b'%' => {
            let count: i64 = payload.parse().context("invalid map length")?;
            let mut map = Vec::new();
            for _ in 0..count {
                let key = Box::pin(read_resp_frame(stream)).await?;
                let value = Box::pin(read_resp_frame(stream)).await?;
                map.push((key, value));
            }
            Ok(RespFrame::Map(map))
        }
        b'~' => {
            let count: i64 = payload.parse().context("invalid set length")?;
            let mut set = Vec::new();
            for _ in 0..count {
                set.push(Box::pin(read_resp_frame(stream)).await?);
            }
            Ok(RespFrame::Array(set))
        }
        _ => Ok(RespFrame::Simple(payload.to_string())),
    }
}

pub struct RedisWireDriver {
    stream: Arc<Mutex<Option<TcpStream>>>,
    connected: bool,
    db: u8,
}

impl std::fmt::Debug for RedisWireDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisWireDriver")
            .field("connected", &self.connected)
            .field("db", &self.db)
            .finish()
    }
}

impl RedisWireDriver {
    pub fn new() -> Self {
        Self {
            stream: Arc::new(Mutex::new(None)),
            connected: false,
            db: 0,
        }
    }

    pub fn with_db(db: u8) -> Self {
        Self {
            stream: Arc::new(Mutex::new(None)),
            connected: false,
            db,
        }
    }

    fn encode_command(args: &[&str]) -> Vec<u8> {
        encode_resp_command(args)
    }

    async fn read_frame(stream: &mut TcpStream) -> Result<RespFrame> {
        read_resp_frame(stream).await
    }

    pub async fn send_and_read(&self, args: &[&str]) -> Result<RespFrame> {
        let encoded = Self::encode_command(args);
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().context("not connected")?;
        stream.write_all(&encoded).await?;
        Self::read_frame(stream).await
    }

    /// Convert a RespFrame to a JSON value for QueryRow results.
    fn frame_to_json(frame: &RespFrame) -> serde_json::Value {
        match frame {
            RespFrame::Simple(s) => {
                if let Ok(i) = s.parse::<i64>() {
                    serde_json::Value::Number(i.into())
                } else if let Ok(f) = s.parse::<f64>() {
                    serde_json::json!(f)
                } else {
                    serde_json::Value::String(s.clone())
                }
            }
            RespFrame::Error(e) => serde_json::Value::String(format!("ERROR: {}", e)),
            RespFrame::Integer(i) => serde_json::Value::Number((*i).into()),
            RespFrame::BulkString(data) => {
                if let Ok(s) = std::str::from_utf8(data) {
                    serde_json::Value::String(s.to_string())
                } else {
                    serde_json::Value::String(format!("<{} bytes>", data.len()))
                }
            }
            RespFrame::Null => serde_json::Value::Null,
            RespFrame::Double(f) => serde_json::json!(f),
            RespFrame::BigNumber(i) => serde_json::Value::Number((*i).into()),
            RespFrame::VerbatimString(_, text) => serde_json::Value::String(text.clone()),
            RespFrame::Boolean(b) => serde_json::Value::Bool(*b),
            RespFrame::Array(items) => {
                serde_json::Value::Array(items.iter().map(Self::frame_to_json).collect())
            }
            RespFrame::Map(items) => {
                let mut map = serde_json::Map::new();
                for (k, v) in items {
                    if let Some(key) = k.as_str() {
                        map.insert(key.to_string(), Self::frame_to_json(v));
                    }
                }
                serde_json::Value::Object(map)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum RespFrame {
    Simple(String),
    Error(String),
    Integer(i64),
    BulkString(Vec<u8>),
    Null,
    Double(f64),
    BigNumber(i64),
    VerbatimString(String, String),
    Boolean(bool),
    Array(Vec<RespFrame>),
    Map(Vec<(RespFrame, RespFrame)>),
}

impl RespFrame {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RespFrame::Simple(s) => Some(s),
            RespFrame::BulkString(data) => std::str::from_utf8(data).ok(),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            RespFrame::Integer(i) => Some(*i),
            _ => None,
        }
    }
}

impl RedisWireDriver {
    pub fn kind(&self) -> DriverKind {
        DriverKind::Redis
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub async fn connect(
        &mut self,
        host: &str,
        port: u16,
        _database: &str,
        username: &str,
        password: &str,
    ) -> Result<()> {
        let addr = format!("{}:{}", host, port)
            .to_socket_addrs()?
            .next()
            .context("failed to resolve host")?;

        let stream = TcpStream::connect(addr)
            .await
            .context("failed to connect to redis")?;

        *self.stream.lock().await = Some(stream);

        // Negotiate RESP3
        match self.send_and_read(&["HELLO", "3"]).await? {
            RespFrame::Error(e) => {
                debug!("HELLO 3 not supported ({}), using RESP2", e);
            }
            RespFrame::Map(_) => {
                debug!("RESP3 negotiated via HELLO 3");
            }
            _ => {
                debug!("HELLO 3 response unexpected, continuing");
            }
        }

        // Authenticate
        if !password.is_empty() {
            if !username.is_empty() {
                match self.send_and_read(&["AUTH", username, password]).await? {
                    RespFrame::Simple(s) if s == "OK" => {
                        debug!("AUTH successful with username");
                    }
                    RespFrame::Error(e) => bail!("AUTH failed: {}", e),
                    _ => {}
                }
            } else {
                match self.send_and_read(&["AUTH", password]).await? {
                    RespFrame::Simple(s) if s == "OK" => {
                        debug!("AUTH successful");
                    }
                    RespFrame::Error(e) => bail!("AUTH failed: {}", e),
                    _ => {}
                }
            }
        }

        // Select database
        if self.db > 0 {
            match self
                .send_and_read(&["SELECT", &self.db.to_string()])
                .await?
            {
                RespFrame::Simple(s) if s == "OK" => {
                    debug!(db = self.db, "selected database");
                }
                RespFrame::Error(e) => bail!("SELECT failed: {}", e),
                _ => {}
            }
        }

        self.connected = true;
        info!(host, port, db = self.db, "redis wire connected");
        Ok(())
    }

    /// For Redis, the `sql` field is the Redis command (e.g. "GET", "SET", "DEL"),
    /// and `params` are the arguments. Returns a single-column "value" result.
    #[instrument(skip(self, params), fields(sql))]
    pub async fn query(&self, sql: &str, params: &[serde_json::Value]) -> Result<QueryRow> {
        let mut args: Vec<String> = Vec::with_capacity(1 + params.len());
        args.push(sql.to_string());
        for p in params {
            args.push(match p {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        let frame = self.send_and_read(&arg_refs).await?;

        if let RespFrame::Error(e) = &frame {
            bail!("Redis error: {}", e);
        }

        let json_val = Self::frame_to_json(&frame);
        Ok(QueryRow {
            columns: vec!["value".to_string()],
            values: vec![json_val],
        })
    }

    /// For Redis, execute returns 1 for OK/integer responses, 0 otherwise.
    #[instrument(skip(self, params), fields(sql))]
    pub async fn execute(&self, sql: &str, params: &[serde_json::Value]) -> Result<u64> {
        let mut args: Vec<String> = Vec::with_capacity(1 + params.len());
        args.push(sql.to_string());
        for p in params {
            args.push(match p {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        let frame = self.send_and_read(&arg_refs).await?;

        match &frame {
            RespFrame::Error(e) => bail!("Redis error: {}", e),
            RespFrame::Simple(s) if s == "OK" => Ok(1),
            RespFrame::Integer(i) => Ok(*i as u64),
            _ => Ok(0),
        }
    }

    pub async fn ping(&self) -> Result<()> {
        if !self.connected {
            bail!("not connected");
        }
        match self.send_and_read(&["PING"]).await? {
            RespFrame::Simple(s) if s == "PONG" || s == "OK" => Ok(()),
            RespFrame::Error(e) => bail!("PING failed: {}", e),
            other => bail!("unexpected PING response: {:?}", other),
        }
    }
}

impl Default for RedisWireDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RedisWireDriver {
    fn clone(&self) -> Self {
        Self {
            stream: Arc::clone(&self.stream),
            connected: self.connected,
            db: self.db,
        }
    }
}
