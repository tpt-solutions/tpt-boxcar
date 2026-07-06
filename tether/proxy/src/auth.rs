use std::collections::HashMap;
use std::env;
use std::fmt;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DbCredentials {
    #[serde(rename = "direct")]
    Direct {
        username: String,
        password: String,
    },
    #[serde(rename = "env")]
    Env {
        username_var: String,
        password_var: String,
    },
}

impl DbCredentials {
    pub fn username(&self) -> Result<String> {
        match self {
            DbCredentials::Direct { username, .. } => Ok(username.clone()),
            DbCredentials::Env { username_var, .. } => env::var(username_var)
                .with_context(|| format!("env var `{}` not set", username_var)),
        }
    }

    pub fn password(&self) -> Result<String> {
        match self {
            DbCredentials::Direct { password, .. } => Ok(password.clone()),
            DbCredentials::Env {
                username_var: _,
                password_var,
            } => {
                let password = env::var(password_var)
                    .with_context(|| format!("env var `{}` not set", password_var))?;
                if password.is_empty() {
                    warn!(var = %password_var, "password env var is empty");
                }
                Ok(password)
            }
        }
    }

    pub fn resolve(&self) -> Result<ResolvedCredentials> {
        let username = match self {
            DbCredentials::Direct { username, .. } => username.clone(),
            DbCredentials::Env { username_var, .. } => env::var(username_var)
                .with_context(|| format!("env var `{}` not set", username_var))?,
        };
        let password = self.password()?;
        debug!(username = %username, "credentials resolved");
        Ok(ResolvedCredentials { username, password })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedCredentials {
    pub username: String,
    pub password: String,
}

impl ResolvedCredentials {
    pub fn connection_string_fragment(&self) -> String {
        format!("{}:{}", self.username, self.password)
    }
}

/// Identity of the Wasm module/service calling into Tether — matches the
/// key a service is registered under in an Origin manifest's `services`
/// map (e.g. `"api"`), injected as the `TETHER_CALLER_ID` WASI env var.
pub type CallerId = String;

/// Per-caller credential scoping: instead of one `DbCredentials` shared by
/// every caller of a backend, each caller id can be mapped to its own
/// credentials, falling back to `default` for unrecognized/absent callers.
/// Additive over `DbCredentials` — existing single-credential configs keep
/// working unchanged (`overrides` defaults to empty).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedCredentials {
    pub default: DbCredentials,
    #[serde(default)]
    pub overrides: HashMap<CallerId, DbCredentials>,
}

impl ScopedCredentials {
    pub fn new(default: DbCredentials) -> Self {
        Self {
            default,
            overrides: HashMap::new(),
        }
    }

    pub fn resolve_for(&self, caller: Option<&str>) -> Result<ResolvedCredentials> {
        match caller.and_then(|c| self.overrides.get(c)) {
            Some(creds) => creds.resolve(),
            None => self.default.resolve(),
        }
    }
}

impl fmt::Display for DbCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbCredentials::Direct { username, .. } => {
                write!(f, "Direct({})", username)
            }
            DbCredentials::Env {
                username_var,
                password_var,
            } => {
                write!(f, "Env({}, {})", username_var, password_var)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_credentials() {
        let creds = DbCredentials::Direct {
            username: "admin".into(),
            password: "secret".into(),
        };
        assert_eq!(creds.password().unwrap(), "secret");
    }

    #[test]
    fn test_env_credentials() {
        env::set_var("TEST_DB_USER", "testuser");
        env::set_var("TEST_DB_PASS", "testpass");
        let creds = DbCredentials::Env {
            username_var: "TEST_DB_USER".into(),
            password_var: "TEST_DB_PASS".into(),
        };
        let resolved = creds.resolve().unwrap();
        assert_eq!(resolved.username, "testuser");
        assert_eq!(resolved.password, "testpass");
    }

    #[test]
    fn scoped_credentials_resolves_override_for_known_caller() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "module-a".to_string(),
            DbCredentials::Direct {
                username: "module-a-user".into(),
                password: "module-a-pass".into(),
            },
        );
        let scoped = ScopedCredentials {
            default: DbCredentials::Direct {
                username: "default-user".into(),
                password: "default-pass".into(),
            },
            overrides,
        };

        let resolved = scoped.resolve_for(Some("module-a")).unwrap();
        assert_eq!(resolved.username, "module-a-user");
        assert_eq!(resolved.password, "module-a-pass");
    }

    #[test]
    fn scoped_credentials_falls_back_to_default_for_unknown_or_absent_caller() {
        let scoped = ScopedCredentials::new(DbCredentials::Direct {
            username: "default-user".into(),
            password: "default-pass".into(),
        });

        let resolved_unknown = scoped.resolve_for(Some("unknown-module")).unwrap();
        assert_eq!(resolved_unknown.username, "default-user");

        let resolved_none = scoped.resolve_for(None).unwrap();
        assert_eq!(resolved_none.username, "default-user");
    }
}
