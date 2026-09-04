use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response};
use tracing::debug;

use crate::loadbalancer::LoadBalancer;

#[derive(Debug, Clone)]
pub struct RouteRule {
    pub path_prefix: Option<String>,
    pub headers: HashMap<String, String>,
    pub upstream: String,
    pub priority: u32,
    pub strip_prefix: bool,
}

pub struct Router {
    rules: Vec<RouteRule>,
    upstreams: HashMap<String, Arc<dyn LoadBalancer>>,
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Router {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            upstreams: HashMap::new(),
        }
    }

    pub fn add_rule(&mut self, rule: RouteRule) {
        self.rules.push(rule);
        self.rules.sort_by_key(|b| std::cmp::Reverse(b.priority));
    }

    pub fn register_upstream(&mut self, name: String, lb: Arc<dyn LoadBalancer>) {
        self.upstreams.insert(name, lb);
    }

    pub async fn route(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<Full<Bytes>>> {
        let matched = self.find_rule(&req);
        match matched {
            Some(rule) => {
                debug!("matched rule: upstream={}", rule.upstream);
                let path = req.uri().path().to_string();
                let modified_path = if rule.strip_prefix {
                    rule.path_prefix
                        .as_ref()
                        .map(|prefix| path.strip_prefix(prefix).unwrap_or(&path).to_string())
                        .unwrap_or(path)
                } else {
                    path
                };

                let (parts, body) = req.into_parts();
                let body_bytes = http_body_util::BodyExt::collect(body)
                    .await
                    .context("failed to read request body")?
                    .to_bytes();

                let lb = self
                    .upstreams
                    .get(&rule.upstream)
                    .context(format!("upstream not found: {}", rule.upstream))?;

                let endpoint = lb.next_endpoint().context("no available endpoints")?;
                let uri = format!("http://{}{}", endpoint.addr, modified_path);

                let mut upstream_req = Request::builder()
                    .method(parts.method)
                    .uri(uri)
                    .version(parts.version);

                for (key, value) in parts.headers.iter() {
                    upstream_req = upstream_req.header(key, value);
                }

                let upstream_req = upstream_req
                    .body(Full::new(body_bytes))
                    .context("failed to build upstream request")?;

                let client =
                    hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build_http();

                let resp = client
                    .request(upstream_req)
                    .await
                    .context("upstream request failed")?;

                let (resp_parts, resp_body) = resp.into_parts();
                let resp_bytes = http_body_util::BodyExt::collect(resp_body)
                    .await
                    .context("failed to read upstream response")?
                    .to_bytes();

                let response = Response::builder()
                    .status(resp_parts.status)
                    .version(resp_parts.version)
                    .body(Full::new(resp_bytes))
                    .context("failed to build response")?;

                lb.mark_healthy(&endpoint);
                Ok(response)
            }
            None => {
                let mut response = Response::new(Full::new(Bytes::from("no matching route")));
                *response.status_mut() = hyper::StatusCode::NOT_FOUND;
                Ok(response)
            }
        }
    }

    fn find_rule(&self, req: &Request<hyper::body::Incoming>) -> Option<&RouteRule> {
        let path = req.uri().path();

        for rule in &self.rules {
            if let Some(ref prefix) = rule.path_prefix {
                if !path.starts_with(prefix) {
                    continue;
                }
            }

            let mut headers_match = true;
            for (key, value) in &rule.headers {
                match req.headers().get(key) {
                    Some(header_val) => {
                        if header_val.to_str().unwrap_or("") != value.as_str() {
                            headers_match = false;
                            break;
                        }
                    }
                    None => {
                        headers_match = false;
                        break;
                    }
                }
            }

            if headers_match {
                return Some(rule);
            }
        }

        None
    }
}

use hyper_util::rt::TokioExecutor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_new() {
        let router = Router::new();
        assert!(router.rules.is_empty());
    }
}
