#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
    pub auth: AuthConfig,
    pub features: FeatureFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: Option<usize>,
    pub timeout_seconds: u64,
    pub max_connections: usize,
    pub tls: Option<TlsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
    pub ca_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connection_timeout: u64,
    pub idle_timeout: u64,
    pub migrations_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: LogFormat,
    pub output: LogOutput,
    pub file_path: Option<String>,
    pub rotation: Option<LogRotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogFormat {
    Json,
    Plain,
    Structured,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogOutput {
    Stdout,
    File,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRotation {
    pub max_size_mb: u64,
    pub max_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub jwt_expiration_hours: u64,
    pub bcrypt_cost: u32,
    pub session_timeout_minutes: u64,
    pub max_login_attempts: u32,
    pub lockout_duration_minutes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    pub enable_metrics: bool,
    pub enable_tracing: bool,
    pub enable_rate_limiting: bool,
    pub enable_caching: bool,
    pub debug_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            logging: LoggingConfig::default(),
            auth: AuthConfig::default(),
            features: FeatureFlags::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            workers: None,
            timeout_seconds: 30,
            max_connections: 1000,
            tls: None,
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "sqlite://./database.db".to_string(),
            max_connections: 10,
            min_connections: 1,
            connection_timeout: 30,
            idle_timeout: 600,
            migrations_path: Some("./migrations".to_string()),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Plain,
            output: LogOutput::Stdout,
            file_path: None,
            rotation: None,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: "your-secret-key-change-this".to_string(),
            jwt_expiration_hours: 24,
            bcrypt_cost: 12,
            session_timeout_minutes: 30,
            max_login_attempts: 5,
            lockout_duration_minutes: 15,
        }
    }
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            enable_metrics: false,
            enable_tracing: false,
            enable_rate_limiting: true,
            enable_caching: true,
            debug_mode: false,
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    JsonParse(serde_json::Error),
    EnvVar(env::VarError),
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(err) => write!(f, "IO error: {}", err),
            ConfigError::JsonParse(err) => write!(f, "JSON parsing error: {}", err),
            ConfigError::EnvVar(err) => write!(f, "Environment variable error: {}", err),
            ConfigError::Invalid(msg) => write!(f, "Invalid configuration: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError::Io(err)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(err: serde_json::Error) -> Self {
        ConfigError::JsonParse(err)
    }
}

impl From<env::VarError> for ConfigError {
    fn from(err: env::VarError) -> Self {
        ConfigError::EnvVar(err)
    }
}

impl Config {
    /// Load configuration from file with environment variable overrides
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "config.json".to_string());
        
        let mut config = if Path::new(&config_path).exists() {
            Self::from_file(&config_path)?
        } else {
            Self::default()
        };

        // Apply environment variable overrides
        config.apply_env_overrides()?;
        
        // Validate configuration
        config.validate()?;
        
        Ok(config)
    }

    /// Load configuration from a specific file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(&path)?;
        let extension = path.as_ref()
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("json");

        match extension.to_lowercase().as_str() {
            "json" => Ok(serde_json::from_str(&content)?),
            _ => Err(ConfigError::Invalid("Only JSON format is supported".to_string())),
        }
    }

    /// Apply environment variable overrides
    fn apply_env_overrides(&mut self) -> Result<(), ConfigError> {
        if let Ok(host) = env::var("SERVER_HOST") {
            self.server.host = host;
        }
        
        if let Ok(port) = env::var("SERVER_PORT") {
            self.server.port = port.parse()
                .map_err(|_| ConfigError::Invalid("Invalid SERVER_PORT".to_string()))?;
        }

        if let Ok(db_url) = env::var("DATABASE_URL") {
            self.database.url = db_url;
        }

        if let Ok(log_level) = env::var("LOG_LEVEL") {
            self.logging.level = log_level;
        }

        if let Ok(jwt_secret) = env::var("JWT_SECRET") {
            self.auth.jwt_secret = jwt_secret;
        }

        if let Ok(debug) = env::var("DEBUG") {
            self.features.debug_mode = debug.parse().unwrap_or(false);
        }

        Ok(())
    }

    /// Validate configuration values
    fn validate(&self) -> Result<(), ConfigError> {
        if self.server.port == 0 {
            return Err(ConfigError::Invalid("Server port cannot be 0".to_string()));
        }

        if self.server.max_connections == 0 {
            return Err(ConfigError::Invalid("Max connections cannot be 0".to_string()));
        }

        if self.database.max_connections == 0 {
            return Err(ConfigError::Invalid("Database max connections cannot be 0".to_string()));
        }

        if self.database.min_connections > self.database.max_connections {
            return Err(ConfigError::Invalid(
                "Database min connections cannot exceed max connections".to_string()
            ));
        }

        if self.auth.jwt_secret.len() < 32 {
            return Err(ConfigError::Invalid(
                "JWT secret must be at least 32 characters".to_string()
            ));
        }

        if self.auth.bcrypt_cost < 4 || self.auth.bcrypt_cost > 31 {
            return Err(ConfigError::Invalid(
                "BCrypt cost must be between 4 and 31".to_string()
            ));
        }

        let valid_log_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_log_levels.contains(&self.logging.level.to_lowercase().as_str()) {
            return Err(ConfigError::Invalid(
                format!("Invalid log level: {}", self.logging.level)
            ));
        }

        Ok(())
    }

    /// Save configuration to file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let extension = path.as_ref()
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("json");

        let content = match extension.to_lowercase().as_str() {
            "json" => serde_json::to_string_pretty(self)?,
            _ => return Err(ConfigError::Invalid("Only JSON format is supported".to_string())),
        };

        fs::write(path, content)?;
        Ok(())
    }

    /// Get the server bind address
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }

    /// Check if TLS is enabled
    pub fn is_tls_enabled(&self) -> bool {
        self.server.tls.is_some()
    }

    /// Get log file path if file logging is enabled
    pub fn log_file_path(&self) -> Option<&str> {
        match self.logging.output {
            LogOutput::File | LogOutput::Both => self.logging.file_path.as_deref(),
            LogOutput::Stdout => None,
        }
    }

    /// Check if a feature is enabled
    pub fn is_feature_enabled(&self, feature: &str) -> bool {
        match feature {
            "metrics" => self.features.enable_metrics,
            "tracing" => self.features.enable_tracing,
            "rate_limiting" => self.features.enable_rate_limiting,
            "caching" => self.features.enable_caching,
            "debug" => self.features.debug_mode,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.database.url, "sqlite://./database.db");
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        config.server.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let temp_file = NamedTempFile::new().unwrap();
        
        config.save_to_file(temp_file.path()).unwrap();
        let loaded_config = Config::from_file(temp_file.path()).unwrap();
        
        assert_eq!(config.server.host, loaded_config.server.host);
        assert_eq!(config.server.port, loaded_config.server.port);
    }

    #[test]
    fn test_bind_address() {
        let config = Config::default();
        assert_eq!(config.bind_address(), "127.0.0.1:8080");
    }

    #[test]
    fn test_feature_flags() {
        let config = Config::default();
        assert!(!config.is_feature_enabled("metrics"));
        assert!(config.is_feature_enabled("rate_limiting"));
        assert!(!config.is_feature_enabled("nonexistent"));
    }
}