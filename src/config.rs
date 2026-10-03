use std::fs;
use std::path::Path;
use std::io;
use clap::Args;
use serde::Deserialize;

/// Default config file path (relative to the working directory)
pub const DEFAULT_CONFIG_PATH: &str = "config.toml";

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub proxy: ProxyConfig,
    pub client: ClientConfig,
    pub common: CommonConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommonConfig {
    pub server_addr: String,
    pub server_addr_bind: String,
    pub proxy_addr: String,
    pub proxy_addr_bind: String,
    pub object_size: usize,
    pub storage_size: usize,
    pub crypto_overhead: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub max_read_size: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyConfig {
    pub first_batch_size: usize,
    pub queue_coefficient: usize,
    pub cache_size: u32,
    pub num_threads: usize,
    pub budget_log_file: String,
    pub is_unsafe: bool,
    pub batch_interval_ms: u64,
    pub enable_budget_log: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClientConfig {
    pub num_clients: usize,
    pub req_per_sec: u64,
    pub input_file: String,
    pub output_file: String,
    pub stats_file: String,
    pub enable_output_log: bool,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        toml::from_str(&contents).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn from_file_with_overrides<P: AsRef<Path>>(path: P, overrides: &ConfigOverrides) -> io::Result<Self> {
        let mut config = Self::from_file(path)?;
        config.apply_overrides(overrides);
        Ok(config)
    }

    pub fn from_args(args: &ConfigArgs) -> io::Result<Self> {
        Self::from_file_with_overrides(&args.config, &args.overrides)
    }

    pub fn apply_overrides(&mut self, overrides: &ConfigOverrides) {
        overrides.common.apply(&mut self.common);
        overrides.server.apply(&mut self.server);
        overrides.proxy.apply(&mut self.proxy);
        overrides.client.apply(&mut self.client);
    }
}

#[derive(Debug, Default, Clone, Args)]
pub struct ConfigArgs {
    /// Path to the configuration file
    #[arg(short, long, default_value = DEFAULT_CONFIG_PATH)]
    pub config: String,
    #[command(flatten)]
    pub overrides: ConfigOverrides,
}

#[derive(Debug, Default, Clone, Args)]
pub struct ConfigOverrides {
    #[command(flatten)]
    pub common: CommonOverrides,
    #[command(flatten)]
    pub server: ServerOverrides,
    #[command(flatten)]
    pub proxy: ProxyOverrides,
    #[command(flatten)]
    pub client: ClientOverrides,
}

#[derive(Debug, Default, Clone, Args)]
pub struct CommonOverrides {
    #[arg(long = "common-server-addr")]
    pub server_addr: Option<String>,
    #[arg(long = "common-server-addr-bind")]
    pub server_addr_bind: Option<String>,
    #[arg(long = "common-proxy-addr")]
    pub proxy_addr: Option<String>,
    #[arg(long = "common-proxy-addr-bind")]
    pub proxy_addr_bind: Option<String>,
    #[arg(long = "common-object-size")]
    pub object_size: Option<usize>,
    #[arg(long = "common-storage-size")]
    pub storage_size: Option<usize>,
    #[arg(long = "common-crypto-overhead")]
    pub crypto_overhead: Option<usize>,
}

#[derive(Debug, Default, Clone, Args)]
pub struct ServerOverrides {
    #[arg(long = "server-max-read-size")]
    pub max_read_size: Option<usize>,
}

#[derive(Debug, Default, Clone, Args)]
pub struct ProxyOverrides {
    #[arg(long = "proxy-first-batch-size")]
    pub first_batch_size: Option<usize>,
    #[arg(long = "proxy-queue-coefficient")]
    pub queue_coefficient: Option<usize>,
    #[arg(long = "proxy-cache-size")]
    pub cache_size: Option<u32>,
    #[arg(long = "proxy-num-threads")]
    pub num_threads: Option<usize>,
    #[arg(long = "proxy-budget-log-file")]
    pub budget_log_file: Option<String>,
    #[arg(long = "proxy-is-unsafe")]
    pub is_unsafe: Option<bool>,
    #[arg(long = "proxy-batch-interval-ms")]
    pub batch_interval_ms: Option<u64>,
    #[arg(long = "proxy-enable-budget-log")]
    pub enable_budget_log: Option<bool>,
}

#[derive(Debug, Default, Clone, Args)]
pub struct ClientOverrides {
    #[arg(long = "client-num-clients")]
    pub num_clients: Option<usize>,
    #[arg(long = "client-req-per-sec")]
    pub req_per_sec: Option<u64>,
    #[arg(long = "client-input-file")]
    pub input_file: Option<String>,
    #[arg(long = "client-output-file")]
    pub output_file: Option<String>,
    #[arg(long = "client-stats-file")]
    pub stats_file: Option<String>,
    #[arg(long = "client-enable-output-log")]
    pub enable_output_log: Option<bool>,
}

impl CommonOverrides {
    fn apply(&self, config: &mut CommonConfig) {
        if let Some(value) = &self.server_addr {
            config.server_addr = value.clone();
        }
        if let Some(value) = &self.server_addr_bind {
            config.server_addr_bind = value.clone();
        }
        if let Some(value) = &self.proxy_addr {
            config.proxy_addr = value.clone();
        }
        if let Some(value) = &self.proxy_addr_bind {
            config.proxy_addr_bind = value.clone();
        }
        if let Some(value) = self.object_size {
            config.object_size = value;
        }
        if let Some(value) = self.storage_size {
            config.storage_size = value;
        }
        if let Some(value) = self.crypto_overhead {
            config.crypto_overhead = value;
        }
    }
}

impl ServerOverrides {
    fn apply(&self, config: &mut ServerConfig) {
        if let Some(value) = self.max_read_size {
            config.max_read_size = value;
        }
    }
}

impl ProxyOverrides {
    fn apply(&self, config: &mut ProxyConfig) {
        if let Some(value) = self.first_batch_size {
            config.first_batch_size = value;
        }
        if let Some(value) = self.queue_coefficient {
            config.queue_coefficient = value;
        }
        if let Some(value) = self.cache_size {
            config.cache_size = value;
        }
        if let Some(value) = self.num_threads {
            config.num_threads = value;
        }
        if let Some(value) = &self.budget_log_file {
            config.budget_log_file = value.clone();
        }
        if let Some(value) = self.is_unsafe {
            config.is_unsafe = value;
        }
        if let Some(value) = self.batch_interval_ms {
            config.batch_interval_ms = value;
        }
        if let Some(value) = self.enable_budget_log {
            config.enable_budget_log = value;
        }
    }
}

impl ClientOverrides {
    fn apply(&self, config: &mut ClientConfig) {
        if let Some(value) = self.num_clients {
            config.num_clients = value;
        }
        if let Some(value) = self.req_per_sec {
            config.req_per_sec = value;
        }
        if let Some(value) = &self.input_file {
            config.input_file = value.clone();
        }
        if let Some(value) = &self.output_file {
            config.output_file = value.clone();
        }
        if let Some(value) = &self.stats_file {
            config.stats_file = value.clone();
        }
        if let Some(value) = self.enable_output_log {
            config.enable_output_log = value;
        }
    }
}
