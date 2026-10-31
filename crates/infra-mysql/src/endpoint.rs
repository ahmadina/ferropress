//! Host, user and database name for one connection. The password is never shown.

use std::fmt;

use crate::Error;

/// How to reach one `MySQL` server. Lab credentials stay on localhost.
#[derive(Clone)]
pub struct Endpoint {
    host: Box<str>,
    port: u16,
    user: Box<str>,
    password: Box<str>,
    database: Box<str>,
}

impl Endpoint {
    /// A connection to `host`:`port` as `user`, using `database`.
    #[must_use]
    pub fn new(
        host: impl Into<Box<str>>,
        port: u16,
        user: impl Into<Box<str>>,
        password: impl Into<Box<str>>,
        database: impl Into<Box<str>>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            user: user.into(),
            password: password.into(),
            database: database.into(),
        }
    }

    /// Hostname or IP.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// TCP port.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// User name.
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    /// Password bytes. Callers must not log this.
    #[must_use]
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Default schema.
    #[must_use]
    pub fn database(&self) -> &str {
        &self.database
    }
}

impl TryFrom<&str> for Endpoint {
    type Error = Error;

    /// A `mysql://user:pass@host:port/db` URL. The password is stored, never shown.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the URL is not a `MySQL` connection string.
    fn try_from(url: &str) -> Result<Self, Self::Error> {
        let Ok(opts) = mysql::Opts::from_url(url) else {
            return Err(Error::Unavailable);
        };
        Ok(Self::new(
            opts.get_ip_or_hostname().into_owned(),
            opts.get_tcp_port(),
            opts.get_user().unwrap_or(""),
            opts.get_pass().unwrap_or(""),
            opts.get_db_name().unwrap_or(""),
        ))
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .field("database", &self.database)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::Endpoint;

    #[test]
    fn url_extracts_fields_and_rejects_junk() {
        let parsed = Endpoint::try_from("mysql://reference:secret-pass@127.0.0.1:3306/reference");
        assert!(parsed.is_ok());
        if let Ok(endpoint) = parsed {
            assert_eq!(endpoint.host(), "127.0.0.1");
            assert_eq!(endpoint.port(), 3306);
            assert_eq!(endpoint.password(), "secret-pass");
        }
        assert!(Endpoint::try_from("not-a-url").is_err());
    }

    #[test]
    fn debug_does_not_print_the_password() {
        let endpoint = Endpoint::new("127.0.0.1", 3306, "reference", "secret-pass", "reference");
        let shown = format!("{endpoint:?}");
        assert!(shown.contains("127.0.0.1"));
        assert!(shown.contains("reference"));
        assert!(shown.contains("<redacted>"));
        assert!(!shown.contains("secret-pass"));
        assert_eq!(endpoint.port(), 3306);
        assert_eq!(endpoint.user(), "reference");
        assert_eq!(endpoint.database(), "reference");
        assert_eq!(endpoint.host(), "127.0.0.1");
        assert_eq!(endpoint.password(), "secret-pass");
    }
}
