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
}
