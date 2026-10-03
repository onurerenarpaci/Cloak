use std::io::{self, BufReader, BufWriter, prelude::*};
use std::net::{SocketAddr, TcpListener, TcpStream};
use ndarray::{Array2, s};
use bytemuck::cast_slice;
use clap::Parser;
use cloak::config::{Config, ConfigArgs};
use log::info;

/// Command line arguments for configuring the storage server
#[derive(Parser, Debug)]
struct Args {
    #[command(flatten)]
    config: ConfigArgs,
}

enum RequestType {
    ReadWrite = 0,
    Read = 1,
    Write = 2,
}

/// Storage server that holds encrypted objects in memory
/// Provides batch read/write operations for the ORAM proxy
struct Storage {
    /// 2D array storing encrypted objects, each row is an object
    data: Array2<u8>,
    /// Temporary buffer for processing batch requests
    auxiliary_memory: Vec<u8>
}

impl Storage {
    /// Creates a new storage instance with given dimensions
    /// - storage_size: Number of objects to store
    /// - object_size: Plaintext object size in bytes, as sent by the proxy (excludes the
    ///   encryption overhead; each stored object is `object_size + 32` bytes)
    /// - max_read_size: Maximum number of indices in one request (sizes the index buffer)
    fn new(storage_size: usize, object_size: usize, max_read_size: usize) -> Storage {
        Storage {
            // The +32 must equal the proxy's `crypto_overhead` (16-byte IV + 16-byte GCM tag)
            data: Array2::<u8>::zeros((storage_size, object_size + 32)),
            auxiliary_memory: vec![0u8; max_read_size*4]
        }
    }

    /// Serves requests from the connected proxy until the connection closes or an
    /// invalid request type is received.
    ///
    /// Protocol, after the session header read by `get_config` (integers are
    /// native-endian; every object on the wire is an encrypted object of
    /// `object_size + 32` bytes). Each request consists of:
    /// 1. Request type (1 byte): 0 = read-write, 1 = read, 2 = write
    /// 2. Number of indices `n` (`u32`, at most `max_read_size`)
    /// 3. `n` storage indices (`n` x `u32`)
    /// 4. Depending on the type:
    ///    - 0 (read-write): the server sends the `n` objects in index order, then
    ///      receives `n` objects from the proxy and writes them to the same indices
    ///      (same order)
    ///    - 1 (read): the server sends the `n` objects in index order
    ///    - 2 (write): the server receives `n` objects and writes them to the indices
    /// 5. The server sends a 1-byte acknowledgment (0)
    fn handle_client(&mut self, mut reader: BufReader<TcpStream>, mut writer: BufWriter<TcpStream>) -> io::Result<()> {

        loop {
            // Read request type
            let req_type = self.get_request_type(&mut reader)?;

            match req_type {
                RequestType::ReadWrite => {
                    info!("handling read-write request");
                    let req_length = self.get_request_size(&mut reader)?;
                    info!("request length: {}", req_length);
                    let index_array = self.read_indices(&mut reader, req_length)?;
                    info!("read indices");
                    self.send_requested_objects(&mut writer, &index_array)?;
                    info!("sent requested objects");
                    self.receive_and_write_objects(&mut reader, &index_array)?;
                    
                    writer.write_all(&[0u8])?;
                    writer.flush()?;
                },
                RequestType::Read => {
                    let req_length = self.get_request_size(&mut reader)?;
                    let index_array = self.read_indices(&mut reader, req_length)?;
                    self.send_requested_objects(&mut writer, &index_array)?;
                    
                    writer.write_all(&[0u8])?;
                    writer.flush()?;
                },
                RequestType::Write => {
                    let req_length = self.get_request_size(&mut reader)?;
                    let index_array = self.read_indices(&mut reader, req_length)?;
                    self.receive_and_write_objects(&mut reader, &index_array)?;
                    
                    writer.write_all(&[0u8])?;
                    writer.flush()?;
                }
            }

        }
    }

    fn get_request_type(&self, reader: &mut BufReader<TcpStream>) -> io::Result<RequestType> {
        let mut req_type = [0u8; 1];
        reader.read_exact(&mut req_type)?;
        match req_type[0] {
            0 => Ok(RequestType::ReadWrite),
            1 => Ok(RequestType::Read),
            2 => Ok(RequestType::Write),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid request type")),
        }
    }

    fn get_request_size(&self, reader: &mut BufReader<TcpStream>) -> io::Result<usize> {
        let mut req_size = [0u8; 4];
        reader.read_exact(&mut req_size)?;
        info!("received request size bytes: {:x?}", req_size);
        Ok(u32::from_ne_bytes(req_size) as usize)
    }

    fn read_indices(&mut self, reader: &mut BufReader<TcpStream>, req_length: usize) -> io::Result<Vec<u32>> {
        let index_array = &mut self.auxiliary_memory[..req_length*4];
        reader.read_exact(index_array)?;
        Ok(cast_slice::<u8, u32>(index_array).to_vec())
    }

    fn send_requested_objects(&self, writer: &mut BufWriter<TcpStream>, index_array: &[u32]) -> io::Result<()> {
        
        info!("length of requested indices: {}", index_array.len());
        for index in index_array {
            let object = self.data.slice(s![*index as usize, ..]);
            let object_slice = object.as_slice().unwrap();
            writer.write_all(object_slice)?;
        }
        writer.flush()?;
        Ok(())
    }

    fn receive_and_write_objects(&mut self, reader: &mut BufReader<TcpStream>, index_array: &[u32]) -> io::Result<()> {
        for index in index_array {
            let mut object = self.data.slice_mut(s![*index as usize, ..]);
            let object_slice = object.as_slice_mut().unwrap();
            reader.read_exact(object_slice)?;
        }
        Ok(())
    }
}

/// Reads the session header the proxy sends right after connecting:
/// `storage_size` (`u32`, number of objects) and `object_size` (`u32`, plaintext object
/// size in bytes), both native-endian. Returns (storage_size, object_size)
fn get_config(reader: &mut BufReader<TcpStream>) -> io::Result<(usize, usize)> {
    let mut config = [0u8; 8];
    reader.read_exact(&mut config)?;
    let storage_size = u32::from_ne_bytes(config[..4].try_into().unwrap()) as usize;
    let object_size = u32::from_ne_bytes(config[4..].try_into().unwrap()) as usize;
    Ok((storage_size, object_size))
}

fn main() -> io::Result<()> {
    // Initialize logger with millisecond timestamps; logging is off unless RUST_LOG is set
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("off"))
        .format_timestamp_millis()
        .init();

    let args = Args::parse();
    
    // Read and parse the configuration file, applying any overrides
    let config = Config::from_args(&args.config)?;

    let addr: SocketAddr = config.common.server_addr_bind.parse()
        .map_err(|e| io::Error::other(format!("Invalid server address: {}", e)))?;
    
    let listener = TcpListener::bind(addr)?;

    println!("Starting server on '{}'", addr);

    // Accept proxy connections and serve them one at a time; the server runs until killed
    for stream in listener.incoming().flatten() {
        // Get storage configuration from proxy
        let mut reader = BufReader::new(stream.try_clone()?);
        let writer = BufWriter::new(stream.try_clone()?);
        let (storage_size, object_size) = get_config(&mut reader)?;
        println!("Proxy connected: storage_size={}, object_size={}", storage_size, object_size);

        // Create storage and start handling requests
        let mut storage = Storage::new(storage_size, object_size, config.server.max_read_size);
        let _ = storage.handle_client(reader, writer).map_err(|e| eprintln!("Proxy session ended: {}", e));
    }
    Ok(())
}
