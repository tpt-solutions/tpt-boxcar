pub mod manifest;
pub mod runtime;
pub mod network;
pub mod dns;
pub mod lifecycle;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
