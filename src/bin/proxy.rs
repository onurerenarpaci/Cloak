use cloak::proxy_lib::Proxy;
use cloak::config::{Config, ConfigArgs};
use clap::Parser;
use std::time::Instant;
use std::net::TcpListener;
use mini_moka::sync::Cache;
use log::info;

#[derive(Parser, Debug)]
struct CliArgs {
    #[command(flatten)]
    config: ConfigArgs,
}

/// Main entry point for the Cloak proxy
/// - Initializes proxy with configuration from TOML file
/// - Sets up storage server connection
/// - Accepts and handles one client connection, then exits
fn main() {
    // Initialize logger with millisecond timestamps; logging is off unless RUST_LOG is set
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("off"))
        .format_timestamp_millis()
        .init();

    let cli_args = CliArgs::parse();
    info!("parsed cli args");

    let config = Config::from_args(&cli_args.config).expect("Failed to read config file");
    info!("loaded config from file");

    // Create the proxy cache for recently read/written objects
    // (mini-moka: TinyLFU admission, LRU eviction; capacity in entries)
    let cache = Cache::new(config.proxy.cache_size as u64);
    info!("initialized proxy cache (mini-moka: TinyLFU admission, LRU eviction) with {} entries", config.proxy.cache_size);

    // Initialize proxy and storage
    let proxy = Proxy::new(config.clone());
    info!("initialized proxy with config");
    proxy.storage_init();
    info!("initialized storage server at {}", config.common.server_addr);

    // Start listening for client connections
    let mut start = Instant::now();
    let listener = TcpListener::bind(&config.common.proxy_addr_bind).unwrap();
    println!("listening for clients on {}", &config.common.proxy_addr);

    // Accept and handle one client connection.
    // Note: the proxy serves a single client session and exits when it ends.
    if let Ok((stream, _)) = listener.accept() {
        start = Instant::now();
        Proxy::handle_client_requests(proxy, stream, cache);
    }

    let duration = start.elapsed();
    println!("Time elapsed for the client session: {:?}", duration);
}
