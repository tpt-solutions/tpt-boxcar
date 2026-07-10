//! Real Redis pub/sub, backed by a dedicated connection separate from the
//! pool's query/execute connections.
//!
//! Once a connection issues `SUBSCRIBE`/`PSUBSCRIBE`, Redis restricts it to
//! only `(P)(UNSUBSCRIBE)`/`(P)SUBSCRIBE`/`PING`/`QUIT` — it can no longer be
//! used for `query`/`execute`. So a subscription can't reuse
//! [`super::wire_redis::RedisWireDriver`]'s pooled connection; it opens its
//! own. See `tether/wit/tether.wit`'s `pubsub` interface doc comment for why
//! this is exposed to Wasm guests as a *polling* API rather than a blocking
//! stream: WIT calls are synchronous request/response, so a background task
//! here drains pushed `message`/`pmessage` frames into an in-memory queue,
//! and `poll()` is a non-blocking pop against that queue.

use std::collections::VecDeque;
use std::net::ToSocketAddrs;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use super::wire_redis::{encode_resp_command, read_resp_frame, RespFrame};

#[derive(Debug, Clone)]
pub struct PubSubMessage {
    pub channel: String,
    pub payload: String,
}

pub struct Subscription {
    write_half: Arc<Mutex<OwnedWriteHalf>>,
    inbox: Arc<Mutex<VecDeque<PubSubMessage>>>,
    reader_task: JoinHandle<()>,
}

impl Subscription {
    /// Opens a dedicated connection, authenticates, and issues an initial
    /// `SUBSCRIBE` for `channels` (may be empty — channels can be added
    /// later via `subscribe_more`).
    pub async fn open(
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        channels: &[String],
    ) -> Result<Self> {
        let addr = format!("{host}:{port}")
            .to_socket_addrs()?
            .next()
            .context("failed to resolve host")?;
        let stream = TcpStream::connect(addr)
            .await
            .context("failed to connect to redis for pub/sub")?;
        let (mut read_half, mut write_half) = stream.into_split();

        if !password.is_empty() {
            let args: Vec<&str> = if !username.is_empty() {
                vec!["AUTH", username, password]
            } else {
                vec!["AUTH", password]
            };
            write_half.write_all(&encode_resp_command(&args)).await?;
            match read_resp_frame(&mut read_half).await? {
                RespFrame::Simple(s) if s == "OK" => {}
                RespFrame::Error(e) => bail!("AUTH failed: {e}"),
                _ => {}
            }
        }

        if !channels.is_empty() {
            let mut args = vec!["SUBSCRIBE"];
            args.extend(channels.iter().map(String::as_str));
            write_half.write_all(&encode_resp_command(&args)).await?;
            // One subscribe-confirmation frame arrives per channel.
            for _ in channels {
                read_resp_frame(&mut read_half).await?;
            }
        }

        let inbox: Arc<Mutex<VecDeque<PubSubMessage>>> = Arc::new(Mutex::new(VecDeque::new()));
        let inbox_reader = Arc::clone(&inbox);
        let reader_task = tokio::spawn(async move {
            loop {
                let frame = match read_resp_frame(&mut read_half).await {
                    Ok(f) => f,
                    Err(_) => break, // connection closed/errored — stop draining
                };
                let RespFrame::Array(items) = frame else {
                    continue;
                };
                let kind = items.first().and_then(|f| f.as_str()).unwrap_or_default();
                match kind {
                    "message" if items.len() == 3 => {
                        if let (Some(channel), Some(payload)) =
                            (items[1].as_str(), items[2].as_str())
                        {
                            inbox_reader.lock().await.push_back(PubSubMessage {
                                channel: channel.to_string(),
                                payload: payload.to_string(),
                            });
                        }
                    }
                    "pmessage" if items.len() == 4 => {
                        if let (Some(channel), Some(payload)) =
                            (items[2].as_str(), items[3].as_str())
                        {
                            inbox_reader.lock().await.push_back(PubSubMessage {
                                channel: channel.to_string(),
                                payload: payload.to_string(),
                            });
                        }
                    }
                    // "subscribe"/"unsubscribe"/"psubscribe"/"punsubscribe"
                    // confirmations are acknowledgements, not messages.
                    _ => {}
                }
            }
        });

        Ok(Self {
            write_half: Arc::new(Mutex::new(write_half)),
            inbox,
            reader_task,
        })
    }

    pub async fn subscribe_more(&self, channels: &[String]) -> Result<()> {
        if channels.is_empty() {
            return Ok(());
        }
        let mut args = vec!["SUBSCRIBE"];
        args.extend(channels.iter().map(String::as_str));
        self.write_half
            .lock()
            .await
            .write_all(&encode_resp_command(&args))
            .await?;
        Ok(())
    }

    pub async fn unsubscribe(&self, channels: &[String]) -> Result<()> {
        let mut args = vec!["UNSUBSCRIBE"];
        args.extend(channels.iter().map(String::as_str));
        self.write_half
            .lock()
            .await
            .write_all(&encode_resp_command(&args))
            .await?;
        Ok(())
    }

    /// Non-blocking: returns the next buffered message, if any, without
    /// waiting for one to arrive. Guests are expected to call this in a
    /// loop rather than have the WIT call itself block.
    pub async fn poll(&self) -> Option<PubSubMessage> {
        self.inbox.lock().await.pop_front()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}
