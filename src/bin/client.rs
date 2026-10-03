use tokio::fs::File;
use tokio::io::{self, AsyncWriteExt, BufReader, AsyncBufReadExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use std::collections::{HashMap};
use std::time::Instant;
use std::cmp::min;

use clap::Parser;
use cloak::config::{Config, ConfigArgs};
use cloak::shared_types::{ClientRequest, ClientResponse, Shared, RequestType::{READ, WRITE}};

/// Command line arguments for configuring the client
#[derive(Parser, Debug)]
struct Args {
    #[command(flatten)]
    config: ConfigArgs,
}

/// Spawns client connections that send requests to the proxy
/// Each client has two tasks:
/// 1. Writer task: Sends requests at a fixed rate
/// 2. Reader task: Receives responses and records latencies
async fn spawn_clients(
        config: &Config, 
        reading_receiver: async_channel::Receiver<ClientRequest>, 
        result_sender: mpsc::Sender<String>,
        handles: &mut Vec<JoinHandle<()>>
    ) -> io::Result<()> {

    // Calculate interval between requests to achieve target request rate
    let interval_duration = Duration::from_secs_f64(1.0 / (config.client.req_per_sec as f64 / (config.client.num_clients as f64)));
    
    let addr: SocketAddr = config.common.proxy_addr.parse()
        .map_err(|e| io::Error::other(format!("Invalid proxy address: {}", e)))?;

    for c_id in 0..config.client.num_clients {
        let (mut stream_reader, mut stream_writer) = TcpStream::connect(&addr).await?.into_split();
        let reading_receiver = reading_receiver.clone();
        let result_sender = result_sender.clone();
        // Map to track outstanding requests and their timestamps
        let req_map_1 = Arc::new(Mutex::new(HashMap::<usize, (Option<Instant>, usize)>::new()));
        let req_map_2 = req_map_1.clone();

        // Writer task: Sends requests at fixed intervals
        let writer_handle = tokio::spawn(async move {
            let mut interval = time::interval(interval_duration);
            let mut req_count = 0usize;
            loop {
                interval.tick().await;
                req_count += 1;
                match reading_receiver.recv().await {
                    Ok(mut client_request) => {
                        client_request.request_id = req_count as u32;
                        let request_bytes = client_request.to_bytes();
                        req_map_1.lock().unwrap().insert(req_count, (None, client_request.key as usize));
                        stream_writer.write_all(&request_bytes).await.unwrap();
                        stream_writer.flush().await.unwrap();
                        let time_sent = Instant::now();
                        req_map_1.lock().unwrap().insert(req_count, (Some(time_sent), client_request.key as usize));
                    },
                    Err(_) => {
                        // Workload exhausted: dropping the write half signals the end of the session to the proxy
                        break;
                    }
                }
            }
        });

        let stats_file = config.client.stats_file.clone();
        // Reader task: Receives responses and records latencies
        let reader_handle = tokio::spawn(async move {
            let mut req_count = 0usize;
            let mut req_count_write = 0usize;
            let mut req_count_read = 0usize;
            let mut total_latency = 0u128;
            let mut total_latency_write = 0u128;
            let mut total_latency_read = 0u128;
            let mut latencies: Vec<u64> = Vec::new();
            let start_time = Instant::now();
            // Read responses until the proxy closes the connection
            while let Ok(response) = ClientResponse::read_one_async(&mut stream_reader).await {
                let (time_sent, key) = req_map_2.lock().unwrap().remove(&(response.request_id as usize)).unwrap();
                let duration = time_sent.unwrap_or(Instant::now()).elapsed();
                
                // Record: client_id, request_id, request_type, key, latency, first 10 bytes of response
                result_sender.send(format!("{},{},{:?},{},{},\"{:?}\"\n", 
                    c_id, 
                    response.request_id, 
                    response.request_type, 
                    key, duration.as_millis(), 
                    &response.value[..min(10, response.value.len())])
                ).await.unwrap();
                req_count += 1;
                total_latency += duration.as_millis();
                latencies.push(duration.as_millis() as u64);
                match response.request_type {
                    WRITE => {
                        total_latency_write += duration.as_millis();
                        req_count_write += 1;
                    }
                    READ => {
                        total_latency_read += duration.as_millis();
                        req_count_read += 1;
                    }
                }
            }
            let exp_duration = start_time.elapsed();
            let throughput = (req_count as f64)/(exp_duration.as_secs_f64());
            let avg_latency = (total_latency as f64)/(req_count as f64);
            let avg_latency_write = (total_latency_write as f64)/(req_count_write as f64);
            let avg_latency_read = (total_latency_read as f64)/(req_count_read as f64);
            // Within-run dispersion over the individual query latencies of this run
            let std_latency = (latencies.iter()
                .map(|&l| (l as f64 - avg_latency).powi(2))
                .sum::<f64>()/(req_count as f64)).sqrt();
            latencies.sort_unstable();
            // Nearest-rank p99
            let p99_latency = if latencies.is_empty() {
                f64::NAN
            } else {
                latencies[((latencies.len() as f64 * 0.99).ceil() as usize).saturating_sub(1).min(latencies.len() - 1)] as f64
            };
            let mut stats_writer = File::create(&stats_file).await.unwrap();
            stats_writer.write_all("request_count,throughput,avg_latency,avg_latency_write,avg_latency_read,std_latency,p99_latency\n".as_bytes()).await.unwrap();
            stats_writer.write_all(format!("{},{},{},{},{},{},{}\n", req_count, throughput, avg_latency, avg_latency_write, avg_latency_read, std_latency, p99_latency).as_bytes()).await.unwrap();
        });

        handles.push(writer_handle);
        handles.push(reader_handle);
    }

    Ok(())
}


/// Reads all requests from the workload file into the channel first,
/// then spawns the result writer task.
/// Returns channels for distributing requests and collecting results.
async fn spawn_file_reader_writer(config: &Config, handles: &mut Vec<JoinHandle<()>>) -> (async_channel::Receiver<ClientRequest>, mpsc::Sender<String>) {
    let (reading_sender, reading_receiver) = async_channel::bounded(100000000);
    let (result_sender, mut result_receiver) = mpsc::channel::<String>(100000000);

    // Read all input data into the channel BEFORE starting the experiment
    let input_file = File::open(&config.client.input_file).await.unwrap();
    let mut reader = BufReader::new(input_file);
    let object_size = config.common.object_size;

    let mut buf = String::new();
    let mut request_count = 0usize;
    loop {
        buf.clear();
        reader.read_line(&mut buf).await.unwrap();
        if buf.trim().is_empty() {
            break;
        }
        let mut parts = buf.split_whitespace();
        let key = parts.next().unwrap().parse().unwrap();
        let (request_type, value) = match parts.next() {
            Some(value) => {
                let value = value.parse().unwrap();
                (WRITE, value)
            }
            None => (READ, 0)
        };
        let request = ClientRequest {
            request_id: 0,
            key,
            request_type,
            value: if request_type == READ {Vec::new()} else {vec![(value % 256) as u8; object_size]},
        };
        reading_sender.send(request).await.unwrap();
        request_count += 1;
    }
    println!("Loaded {} requests into channel, starting experiment...", request_count);

    // Task for writing results to output file (only if enabled)
    let enable_output_log = config.client.enable_output_log;
    let output_file = config.client.output_file.clone();
    let writer_handle = tokio::spawn(async move {
        if enable_output_log {
            let mut writer = File::create(&output_file).await.unwrap();
            let csv_header = "client_id,request_id,request_type,key,duration,response\n";
            writer.write_all(csv_header.as_bytes()).await.unwrap();
            while let Some(result) = result_receiver.recv().await {
                writer.write_all(result.as_bytes()).await.unwrap();
            }
        } else {
            // Drain the channel without writing
            while result_receiver.recv().await.is_some() {}
        }
    });

    handles.push(writer_handle);

    (reading_receiver, result_sender)
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    
    // Read and parse the configuration file, then apply any CLI overrides
    let config = Config::from_args(&args.config)?;
    
    let mut handles = Vec::new();

    // Start file I/O tasks and get channels
    let (reading_receiver, result_sender) = spawn_file_reader_writer(&config, &mut handles).await;

    // Start client tasks
    spawn_clients(&config, reading_receiver, result_sender, &mut handles).await?;

    // Wait for all tasks to complete
    for handle in handles {
        handle.await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    /// Unique per-test temporary directory, removed with its contents on drop
    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new(test_name: &str) -> TestDir {
            let dir = std::env::temp_dir()
                .join(format!("cloak-client-test-{}-{}", std::process::id(), test_name));
            std::fs::create_dir_all(&dir).unwrap();
            TestDir(dir)
        }

        fn file(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().into_owned()
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn create_test_config(dir: &TestDir) -> Config {
        Config {
            common: cloak::config::CommonConfig {
                proxy_addr: "127.0.0.1:0".to_string(),
                proxy_addr_bind: "127.0.0.1:0".to_string(),
                server_addr_bind: "127.0.0.1:0".to_string(),
                server_addr: "127.0.0.1:0".to_string(),
                object_size: 100,
                storage_size: 1000,
                crypto_overhead: 32,
            },
            client: cloak::config::ClientConfig {
                num_clients: 1,
                req_per_sec: 10,
                input_file: dir.file("test_input.txt"),
                output_file: dir.file("test_output.csv"),
                stats_file: dir.file("test_stats.csv"),
                enable_output_log: true,
            },
            proxy: cloak::config::ProxyConfig {
                first_batch_size: 100,
                queue_coefficient: 2,
                cache_size: 1000,
                num_threads: 4,
                budget_log_file: dir.file("test_budget.log"),
                is_unsafe: false,
                batch_interval_ms: 100,
                enable_budget_log: true,
            },
            server: cloak::config::ServerConfig {
                max_read_size: 10000,
            },
        }
    }

    #[tokio::test]
    async fn test_spawn_file_reader_writer() -> io::Result<()> {
        // All test files live in a temporary directory that is removed at the end
        let dir = TestDir::new("spawn_file_reader_writer");
        let config = create_test_config(&dir);

        // Write test data to input file
        tokio::fs::write(
            &config.client.input_file,
            "1 42\n2\n3 100\n"
        ).await?;

        let mut handles = Vec::new();
        let (receiver, sender) = spawn_file_reader_writer(&config, &mut handles).await;

        // Test reading requests
        let mut requests = Vec::new();
        for _ in 0..3 {
            if let Ok(req) = receiver.recv().await {
                requests.push(req);
            }
        }

        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].key, 1);
        assert_eq!(requests[0].request_type, WRITE);
        assert_eq!(requests[1].key, 2);
        assert_eq!(requests[1].request_type, READ);
        assert_eq!(requests[2].key, 3);
        assert_eq!(requests[2].request_type, WRITE);

        // Test writing results
        sender.send("1,1,WRITE,1,1000,\"test\"\n".to_string()).await.unwrap();
        drop(sender);

        // Wait for handles to complete
        for handle in handles {
            handle.await?;
        }

        // Verify output file contains header and test data
        let output_contents = tokio::fs::read_to_string(&config.client.output_file).await?;
        assert!(output_contents.contains("client_id,request_id,request_type,key,duration,response\n"));
        assert!(output_contents.contains("1,1,WRITE,1,1000,\"test\"\n"));

        Ok(())
    }

    #[tokio::test]
    async fn test_spawn_clients() -> io::Result<()> {
        // Create a mock server
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let server_addr = listener.local_addr()?;

        // The client's stats file (if written) goes to a temporary directory
        let dir = TestDir::new("spawn_clients");
        let mut config = create_test_config(&dir);
        config.common.proxy_addr = server_addr.to_string();
        config.client.num_clients = 1;
        config.client.req_per_sec = 1;

        let (sender, receiver) = async_channel::bounded(10);
        let (result_sender, mut result_receiver) = mpsc::channel(10);
        let mut handles = Vec::new();

        // Spawn clients
        spawn_clients(&config, receiver, result_sender, &mut handles).await?;

        // Accept connection in mock server
        let (mut socket, _) = listener.accept().await?;
        
        // Send test request
        sender.send(ClientRequest {
            request_id: 0,
            key: 42,
            request_type: READ,
            value: vec![],
        }).await.unwrap();

        // Mock server receives request and sends response
        let mut buf = vec![0u8; 1024];
        let _n = socket.read(&mut buf).await?;
        let response = ClientResponse {
            request_id: 1,
            request_type: READ,
            key: 42,
            value: vec![1, 2, 3, 4, 5],
        };
        socket.write_all(&response.to_bytes()).await?;

        // Verify result is received
        if let Some(result) = result_receiver.recv().await {
            assert!(result.contains("42")); // Contains the key
            assert!(result.contains("[1, 2, 3, 4, 5]")); // Contains response value
        }

        Ok(())
    }
}
