use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct IhaCdnNotifierConfig {
    /// Enable or disable the notifier.
    pub enable: bool,
    /// The Discord webhook URL to send notifications to.
    pub discord_webhook: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IhaCdnPlausibleConfig {
    /// Enable or disable Plausible Analytics.
    pub enable: bool,
    /// The Plausible Analytics domain.
    pub domain: Option<String>,
    /// The Plausible Analytics script URL.
    pub endpoint_url: Option<String>,
}

impl Default for IhaCdnPlausibleConfig {
    fn default() -> Self {
        Self {
            enable: false,
            domain: None,
            endpoint_url: None,
        }
    }
}

impl IhaCdnPlausibleConfig {
    /// Check if Plausible Analytics is enabled and has a domain set.
    pub fn is_enabled(&self) -> bool {
        self.enable && self.domain.is_some()
    }

    /// Get the Plausible Analytics endpoint url.
    pub fn endpoint_url(&self) -> url::Url {
        let endpoint_base = self
            .endpoint_url
            .as_deref()
            .unwrap_or("https://plausible.io");

        let full_path = url::Url::parse(endpoint_base).unwrap_or_else(|_| {
            tracing::warn!("Invalid Plausible Analytics endpoint URL, using default.");
            url::Url::parse("https://plausible.io").unwrap()
        });

        // add path /api/event
        full_path.join("/api/event").unwrap()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IhaCdnRetentionConfig {
    /// Enable or disable the file retention policy.
    pub enable: bool,
    /// The minimum age of files to be deleted. (in days)
    #[serde(default = "default_retention_min_age")]
    pub min_age: u64,
    /// The maximum age of files to be deleted. (in days)
    #[serde(default = "default_retention_max_age")]
    pub max_age: u64,
}

impl Default for IhaCdnRetentionConfig {
    fn default() -> Self {
        Self {
            enable: false,
            min_age: default_retention_min_age(),
            max_age: default_retention_max_age(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IhaCdnStorageConfig {
    /// The maximum file size limit for uploads.
    ///
    /// This is the maximum file size limit for uploads in Kilobytes.
    ///
    /// If this is set to [`None`], there is no limit.
    #[serde(default = "default_filesize_limit")]
    pub filesize_limit: Option<u64>,
    /// The maximum file size limit for uploads for admin
    ///
    /// This is the maximum file size limit for uploads in Kilobytes for admin users.
    ///
    /// If this is set to [`None`], there is no limit.
    pub admin_filesize_limit: Option<u64>,
}

impl Default for IhaCdnStorageConfig {
    fn default() -> Self {
        Self {
            filesize_limit: default_filesize_limit(),
            admin_filesize_limit: None,
        }
    }
}

/// Block certain file extensions and MIME types from being uploaded.
///
/// This will not affect existing files and will not affect admin uploads.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IhaCdnBlocklistConfig {
    /// Block the following file extensions.
    #[serde(rename = "extension", default = "default_block_extension")]
    pub extensions: Vec<String>,
    /// Block the following MIME types.
    #[serde(rename = "content_type", default = "default_block_mimetypes")]
    pub content_types: Vec<String>,
}

impl Default for IhaCdnBlocklistConfig {
    fn default() -> Self {
        Self {
            extensions: default_block_extension(),
            content_types: default_block_mimetypes(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IhaCdnConfig {
    /// The hostname of the IhaCDN server.
    #[serde(default = "default_hostname")]
    pub hostname: String,
    /// The host of the IhaCDN server.
    #[serde(default = "default_hostname")]
    pub host: String,
    /// The port of the IhaCDN server.
    #[serde(default = "default_ihacdn_port")]
    pub port: u16,
    /// HTTPS mode, this only affects the URL generation.
    pub https_mode: bool,
    /// The path to upload files to.
    #[serde(default = "default_ihacdn_upload_path")]
    pub upload_path: String,
    /// Admin password for uploading files.
    #[serde(default = "default_ihacdn_admin_password")]
    pub admin_password: String,
    /// The length of the random filename.
    #[serde(default = "default_filename_length")]
    pub filename_length: usize,
    /// Config for the Redis database.
    pub redis: String,
    /// Config for the notifier.
    pub notifier: IhaCdnNotifierConfig,
    /// Config for the retention policy.
    #[serde(rename = "file_retention")]
    pub retention: IhaCdnRetentionConfig,
    /// Config for the storage.
    pub storage: IhaCdnStorageConfig,
    /// Config for the blocklist.
    pub blocklist: IhaCdnBlocklistConfig,
    /// Config for the Plausible Analytics.
    /// This can be missing if Plausible Analytics is not used.
    #[serde(default)]
    pub plausible: IhaCdnPlausibleConfig,
}

impl Default for IhaCdnConfig {
    fn default() -> Self {
        Self {
            hostname: default_hostname(),
            host: default_hostname(),
            port: default_ihacdn_port(),
            https_mode: false,
            upload_path: default_ihacdn_upload_path(),
            admin_password: default_ihacdn_admin_password(),
            filename_length: default_filename_length(),
            redis: format!("redis://{}:{}", default_hostname(), default_redis_port()),
            notifier: IhaCdnNotifierConfig::default(),
            retention: IhaCdnRetentionConfig::default(),
            storage: IhaCdnStorageConfig::default(),
            blocklist: IhaCdnBlocklistConfig::default(),
            plausible: IhaCdnPlausibleConfig::default(),
        }
    }
}

impl IhaCdnConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the configuration from the `config.json` file.
    pub fn load() -> Self {
        let config = std::fs::read_to_string("config.json");

        match config {
            Ok(config) => {
                let config: IhaCdnConfig = serde_json::from_str(&config).unwrap();
                config
            }
            Err(_) => {
                tracing::warn!("Config file not found, creating a new one.");
                let config = Self::new();
                config.save();
                config
            }
        }
    }

    /// Save the configuration to the `config.json` file.
    pub fn save(&self) {
        let config = serde_json::to_string_pretty(&self).unwrap();
        std::fs::write("config.json", config).unwrap();
    }

    /// Verify if the config is actually valid and correctly set.
    pub fn verify(&self) -> bool {
        if self.hostname.is_empty() {
            tracing::error!("Hostname is empty, please set it in the config file.");
            return false;
        }

        if self.port == 0 {
            tracing::error!("Port is not set, please set it in the config file.");
            return false;
        }

        if self.upload_path.is_empty() {
            tracing::error!("Upload path is empty, please set it in the config file.");
            return false;
        }

        // Verify upload_path exist
        if !std::path::Path::new(&self.upload_path).exists() {
            tracing::error!("Upload path does not exist, please set it in the config file.");
            return false;
        }

        if self.upload_path.is_empty() {
            tracing::error!("Upload path is empty, please set it in the config file.");
            return false;
        }

        // Verify upload_path exist
        let resolved_path = std::fs::canonicalize(&self.upload_path).unwrap();
        if !resolved_path.exists() {
            tracing::error!("Upload path does not exist, please set it in the config file.");
            return false;
        }

        if self.admin_password.is_empty() {
            tracing::error!("Admin password is empty, please set it in the config file.");
            return false;
        }

        if self.filename_length < 5 {
            tracing::error!("Filename length must be longer or equals to 5");
            return false;
        }

        if self.plausible.enable && self.plausible.domain.is_none() {
            tracing::error!("Plausible Analytics is enabled but no domain is set.");
            return false;
        }

        // Create the uploads and uploads_admin dir in upload_path if it's not exist.
        let uploads_path = resolved_path.join("uploads");
        if !uploads_path.exists() {
            std::fs::create_dir_all(&uploads_path).unwrap();
        }
        let uploads_admin_path = resolved_path.join("uploads_admin");
        if !uploads_admin_path.exists() {
            std::fs::create_dir_all(&uploads_admin_path).unwrap();
        }

        true
    }

    pub fn get_path(&self, is_admin: bool) -> PathBuf {
        let mut path = std::fs::canonicalize(&self.upload_path).unwrap();
        if is_admin {
            path.push("uploads_admin");
        } else {
            path.push("uploads");
        }
        path
    }

    pub fn get_limit(&self, is_admin: bool) -> Option<u64> {
        if is_admin {
            self.storage.admin_filesize_limit.map(|limit| limit * 1024)
        } else {
            self.storage.filesize_limit.map(|limit| limit * 1024)
        }
    }

    /// Verify the admin password.
    ///
    /// If the admin password is not changed, this will return `false`.
    ///
    /// ```rust,no_run
    /// use ihacdn::config::IhaCdnConfig;
    ///
    /// let mut config = IhaCdnConfig::new();
    ///
    /// assert_eq!(config.verify_admin_password("mypassword"), false);
    /// config.admin_password = "mypassword".to_string();
    ///
    /// assert_eq!(config.verify_admin_password("mypassword"), true);
    /// assert_eq!(config.verify_admin_password("wrongpassword"), false);
    /// ```
    #[allow(dead_code)]
    pub fn verify_admin_password(&self, password: &str) -> bool {
        if self.admin_password == default_ihacdn_admin_password() {
            tracing::warn!("Admin password is not changed, disabling admin uploads.");
            return false;
        }

        // To avoid timing attacks, we use a constant time comparison.
        let password = password.as_bytes();
        let admin_password = self.admin_password.as_bytes();
        if password.len() != admin_password.len() {
            return false;
        }

        let mut result = 0;
        for (a, b) in password.iter().zip(admin_password.iter()) {
            result |= a ^ b;
        }
        result == 0
    }

    pub fn is_filetype_allowed(&self, filetype: &str) -> bool {
        !self.blocklist.content_types.contains(&filetype.to_string())
    }

    pub fn is_extension_allowed(&self, extension: &str) -> bool {
        !self.blocklist.extensions.contains(&extension.to_string())
    }

    pub fn make_url(&self, file_name: &str) -> String {
        if self.https_mode {
            format!("https://{}/{}", self.hostname, file_name)
        } else {
            format!("http://{}/{}", self.hostname, file_name)
        }
    }
}

fn default_hostname() -> String {
    "127.0.0.1".to_string()
}

fn default_redis_port() -> u16 {
    6379
}

fn default_ihacdn_port() -> u16 {
    6969
}

fn default_ihacdn_upload_path() -> String {
    "./".to_string()
}

fn default_ihacdn_admin_password() -> String {
    "PLEASE_CHANGE_THIS".to_string()
}

fn default_filename_length() -> usize {
    8
}

fn default_retention_min_age() -> u64 {
    30
}

fn default_retention_max_age() -> u64 {
    180
}

fn default_filesize_limit() -> Option<u64> {
    // 512mb
    Some(524288)
}

fn default_block_extension() -> Vec<String> {
    vec![
        "exe".to_string(),
        "sh".to_string(),
        "msi".to_string(),
        "bat".to_string(),
        "dll".to_string(),
        "com".to_string(),
    ]
}

fn default_block_mimetypes() -> Vec<String> {
    vec![
        "text/x-sh".to_string(),
        "text/x-msdos-batch".to_string(),
        "application/x-dosexec".to_string(),
        "application/x-msdownload".to_string(),
        "application/vnd.microsoft.portable-executable".to_string(),
        "application/x-msi".to_string(),
        "application/x-msdos-program".to_string(),
        "application/x-sh".to_string(),
    ]
}
