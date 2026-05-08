use std::env;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub files_dir: PathBuf,
    pub bind_addr: String,
    pub dev_viewer_email: Option<String>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &mask_url_password(&self.database_url))
            .field("files_dir", &self.files_dir)
            .field("bind_addr", &self.bind_addr)
            .field("dev_viewer_email", &self.dev_viewer_email)
            .finish()
    }
}

/// Replace the password between `://user:` and `@host` with `***`.
fn mask_url_password(url: &str) -> String {
    let scheme_end = match url.find("://") {
        Some(i) => i + 3,
        None => return url.to_string(),
    };
    let rest = &url[scheme_end..];
    let at = match rest.find('@') {
        Some(i) => i,
        None => return url.to_string(),
    };
    let userinfo = &rest[..at];
    let host_and_after = &rest[at..];
    let masked_userinfo = match userinfo.find(':') {
        Some(c) => format!("{}:***", &userinfo[..c]),
        None => userinfo.to_string(),
    };
    format!("{}{}{}", &url[..scheme_end], masked_userinfo, host_and_after)
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            database_url: env::var("DATABASE_URL")
                .map_err(|_| "DATABASE_URL is required".to_string())?,
            files_dir: PathBuf::from(
                env::var("FILES_DIR").map_err(|_| "FILES_DIR is required".to_string())?,
            ),
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            dev_viewer_email: env::var("DEV_VIEWER_EMAIL").ok(),
        })
    }
}
