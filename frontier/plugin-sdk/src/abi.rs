use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterResult {
    Continue,
    ModifyRequest(HttpRequest),
    ModifyResponse(HttpResponse),
    Deny { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContext {
    pub request: HttpRequest,
    pub metadata: HashMap<String, String>,
}

pub const ABI_VERSION: u32 = 1;

pub trait PluginFilter {
    fn on_request(&self, request: &HttpRequest) -> FilterResult;
    fn on_response(&self, request: &HttpRequest, response: &HttpResponse) -> FilterResult;
}

pub trait PluginHost {
    fn log(&self, level: LogLevel, message: &str);
    fn get_shared_data(&self, key: &str) -> Option<Vec<u8>>;
    fn set_shared_data(&self, key: &str, value: &[u8]);
    fn get_config(&self, key: &str) -> Option<String>;
    fn http_request(&self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub abi_version: u32,
    pub metadata: PluginMetadata,
    pub permissions: Vec<String>,
    pub config_schema: HashMap<String, ConfigField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigField {
    pub field_type: String,
    pub required: bool,
    pub default: Option<String>,
    pub description: String,
}

pub const HOST_FUNCTIONS: &[&str] = &[
    "host_log",
    "host_get_shared_data",
    "host_set_shared_data",
    "host_get_config",
    "host_http_request",
];

pub const GUEST_FUNCTIONS: &[&str] = &[
    "guest_init",
    "guest_on_request",
    "guest_on_response",
    "guest_shutdown",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCall {
    pub function: String,
    pub args: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostResponse {
    pub success: bool,
    pub data: Vec<u8>,
    pub error: Option<String>,
}
