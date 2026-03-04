use std::time::Duration;

const INSECURE_TLS_ENV_GUARD: &str = "POSTERM_ALLOW_INSECURE_TLS";

#[derive(Debug, Clone)]
pub struct HttpClientPool {
    strict: reqwest::Client,
    permissive: reqwest::Client,
}

impl HttpClientPool {
    pub fn new() -> Result<Self, reqwest::Error> {
        let strict = build_client(false)?;
        let permissive = build_client(true)?;
        Ok(Self { strict, permissive })
    }

    pub fn client(&self, permissive_tls: bool) -> Result<&reqwest::Client, InsecureTlsGuardError> {
        if permissive_tls && !allows_insecure_tls() {
            return Err(InsecureTlsGuardError::NotAllowed);
        }

        if permissive_tls {
            Ok(&self.permissive)
        } else {
            Ok(&self.strict)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsecureTlsGuardError {
    NotAllowed,
}

impl std::fmt::Display for InsecureTlsGuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAllowed => write!(
                f,
                "insecure TLS is disabled; set {INSECURE_TLS_ENV_GUARD}=1 to enable intentionally"
            ),
        }
    }
}

impl std::error::Error for InsecureTlsGuardError {}

fn allows_insecure_tls() -> bool {
    matches!(
        std::env::var(INSECURE_TLS_ENV_GUARD).ok().as_deref(),
        Some("1")
    )
}

fn build_client(permissive_tls: bool) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .tcp_keepalive(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10));

    if permissive_tls {
        builder = builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::{HttpClientPool, INSECURE_TLS_ENV_GUARD};

    #[test]
    fn blocks_permissive_tls_without_env_guard() {
        // SAFETY: tests run in-process; this test restores env state before exiting.
        let original = std::env::var_os(INSECURE_TLS_ENV_GUARD);
        unsafe { std::env::remove_var(INSECURE_TLS_ENV_GUARD) };

        let pool = HttpClientPool::new().expect("pool should initialize");
        let result = pool.client(true);

        if let Some(value) = original {
            // SAFETY: restoring prior value.
            unsafe { std::env::set_var(INSECURE_TLS_ENV_GUARD, value) };
        }

        assert!(result.is_err());
    }
}
