use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{SyncSender, Receiver};
use rand::seq::{SliceRandom};
use std::io::{BufReader, BufWriter, prelude::*};
use rand::{rng, SeedableRng};
use rand::rngs::SmallRng;
use std::thread;
use std::sync::{Arc, Mutex, mpsc::sync_channel};
use std::net::{TcpStream};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use ndarray::{Array2, s, Axis};
use mini_moka::sync::Cache;
use crate::budget_queue::HeapPQ;
use crate::config::Config;
use std::fs::File;
use crate::shared_types::{ClientRequest, ClientResponse, Shared, RequestType::{self, READ, WRITE}};
use log::{info, error, trace};

pub mod crypto_lib;
use crypto_lib::Encryptor;

/// Pmap (Position Map) maintains the mapping between logical and physical locations
/// of objects in the ORAM storage as well as the timestamp of the last access
#[derive(Debug)]
pub struct Pmap {
    // Maps logical indices to physical storage locations
    forward: Vec<usize>,
    // Maps physical storage locations back to logical indices
    reverse: Vec<usize>,
    // Tracks when each logical location was last accessed (real or dummy access)
    time_stamps_observed: Vec<usize>,
    // Tracks when each logical location was last accessed by a real request
    time_stamps_real: Vec<usize>,
}

impl Pmap {
    /// Creates a new position map with size 's'
    pub fn new(s: usize) -> Pmap {
        let forward: Vec<usize> = (0..s).collect();
        let mut reverse = vec![0usize; s];
        let time_stamps_observed = vec![0usize; s];
        let time_stamps_real = vec![0usize; s];
        for i in 0..s {
            reverse[forward[i]] = i;
        }
        Pmap { forward, reverse, time_stamps_observed, time_stamps_real }
    }

    /// Updates the mapping for a given logical key to point to a new physical location
    /// Also updates the timestamp of the access
    pub fn set_key(&mut self, key: usize, value: usize, time_stamp_observed: usize, is_real: bool) {
        self.forward[key] = value;
        self.reverse[value] = key;
        self.time_stamps_observed[key] = time_stamp_observed;
        if is_real {
            self.time_stamps_real[key] = time_stamp_observed;
        }
    }
}

/// TimeSets maintains sets of storage locations grouped by their access timestamps
/// Each time-set represents locations accessed at the same time, with a fixed budget
/// of how many locations can be accessed from that set in future operations
#[derive(Debug)]
pub struct TimeSets {
    // List of (set, queue) pairs where:
    // - set: contains all locations in this time period
    // - queue: tracks pending requests for locations in this set
    set_list: VecDeque<(HeapPQ<usize, usize>, VecDeque<(usize, RequestType)>)>,
    // Number of locations that can be accessed from each time-set
    budgets: Vec<usize>,
}

impl TimeSets {
    /// Creates a new TimeSets structure with specified budgets and initializes
    /// random distribution of locations across time-sets
    pub fn new(budgets: Vec<usize>, n: usize, p_map: &mut Pmap) -> TimeSets {
        let mut budget_total = 0;
        for i in 0..budgets.len() {
            budget_total += budgets[i]*(1+i);
        }
        assert!(budget_total == n, "Budgets do not add up to n");
        
        let t = budgets.len();
        let mut random_time_sets: Vec<usize> = (0..n).collect();
        random_time_sets.shuffle(&mut rng());

        let mut time_sets = TimeSets {
            set_list: (0..t).map(|_| (HeapPQ::new(), VecDeque::new())).collect(),
            budgets,
        };

        let mut current_time_set_size = 0usize;
        let mut current_total_size = 0usize;
        for i in (0..t).rev() {
            current_time_set_size += time_sets.budgets[i];
            
            for j in current_total_size..(current_total_size + current_time_set_size){
                time_sets.set_list[i].0.insert(0, random_time_sets[j]);
                p_map.time_stamps_observed[random_time_sets[j]] = t - i - 1;
            }
            current_total_size += current_time_set_size;
        }
        assert!(time_sets.set_list[0].0.len() == time_sets.budgets.iter().sum::<usize>(), "Set 0 is not the same as the sum of budgets");
        time_sets
    }
    
    /// Adds a new time-set to the front of the queue and removes the oldest one
    pub fn add_time_set(&mut self, new_time_set: HeapPQ<usize, usize>) {
        self.set_list.push_front((new_time_set, VecDeque::new()));
        self.set_list.pop_back();
    }

    /// Returns mutable references to a time-set's components at given index
    pub fn index_mut(&mut self, idx: usize) -> (&mut HeapPQ<usize, usize>, &mut VecDeque<(usize, RequestType)>, usize) {
        let (set, queue) = &mut self.set_list[idx];
        (set, queue, self.budgets[idx])
    }

    /// Adds a new request to the queue of a specific time-set
    pub fn insert(&mut self, key: usize, set_idx: usize, request_type: RequestType) {
        match request_type {
            READ => self.set_list[set_idx].1.push_back((key, READ)),
            WRITE => self.insert_write(key, set_idx),
        }
    }

    pub fn insert_write(&mut self, key: usize, set_idx: usize) {
        // check if the key is in the queue if it is change the request type to write
        for (i, (k, _r)) in self.set_list[set_idx].1.iter().enumerate() {
            if *k == key {
                self.set_list[set_idx].1.get_mut(i).unwrap().1 = WRITE;
                return;
            }
        }
        // if the key is not in the queue, add it to the queue
        self.set_list[set_idx].1.push_back((key, WRITE));
    }
}

/// Calculates budget distribution for time-sets using Zipf's law
/// Returns budgets for each time period and total storage size needed
pub fn zipf_budgets(n: usize, first: usize) -> (Vec<usize>, usize) {
    let mut budgets = Vec::new();
    let mut total = 0;
    let mut i = 1;
    while total < n {
        let budget = (first as f64 / i as f64).ceil() as usize;
        total += budget * i;
        budgets.push(budget);
        i += 1;
    }
    (budgets, total)
}

/// Main proxy structure that implements the ORAM protocol
/// Handles client requests, maintains access pattern privacy, and communicates with storage
#[derive(Debug)]
pub struct Proxy {
    pmap: Pmap,
    time_sets: TimeSets,
    config: Config,
    encrypted_size: usize,
    connection: TcpStream,
    calc_storage_size: usize
}

impl Proxy {
    /// Creates a new proxy instance with specified arguments
    /// Initializes position map and time-sets with Zipf-distributed budgets
    pub fn new(config: Config) -> Proxy {
        let (budgets, calc_size) = zipf_budgets(config.common.storage_size, config.proxy.first_batch_size);
        let mut pmap = Pmap::new(calc_size);
        let time_sets = TimeSets::new(budgets, calc_size, &mut pmap);
        Proxy {
            pmap,
            time_sets,
            connection: TcpStream::connect(&config.common.server_addr).unwrap(),
            encrypted_size: config.common.object_size + config.common.crypto_overhead,
            config,
            calc_storage_size: calc_size,
        }
    }

    /// Continuously reads client requests from TCP stream
    /// Checks cache for requested objects before forwarding to main protocol
    pub fn get_client_requests(
        reader: &mut BufReader<TcpStream>,  
        cache: Cache<usize, Vec<u8>>, 
        req_sender: SyncSender<Option<ClientRequest>>,
        resp_sender: SyncSender<ClientResponse>
        ){

        loop {
            let client_request = ClientRequest::read_one(reader);
            match client_request {
                Ok(client_request) => {
                    trace!("Received request: id={}, key={}, type={:?}", 
                          client_request.request_id, client_request.key, client_request.request_type);

                    match client_request.request_type {
                        READ =>{
                            match cache.get(&(client_request.key as usize)) {
                                Some(resp) => {
                                    trace!("Cache hit for READ request: id={}", client_request.request_id);
                                    resp_sender.send(ClientResponse{
                                        request_id: client_request.request_id, 
                                        request_type: READ, 
                                        key: client_request.key, 
                                        value: resp.clone()
                                    }).unwrap();
                                },
                                None => {
                                    trace!("Cache miss for READ request: id={}", client_request.request_id);
                                    req_sender.send(Some(client_request.clone())).unwrap();
                                }
                            }
                        },
                        WRITE =>{
                            trace!("Cache inserted for WRITE request, sent to consumer, and responded to client: id={}", client_request.request_id);
                            cache.insert(client_request.key as usize, client_request.value.clone());
                            req_sender.send(Some(client_request.clone())).unwrap();
                            resp_sender.send(ClientResponse{
                                request_id: client_request.request_id, 
                                request_type: WRITE, 
                                key: client_request.key, 
                                value: Vec::new()
                            }).unwrap();
                        }
                    }
                },
                Err(e) => {
                    error!("Error reading client request: {:?}", e);
                    break;
                }
            }
        }

        info!("No more requests, sending termination signal");
        req_sender.send(None).unwrap();
    }

    /// Main request handling loop that handles the io operations and the protocol logic in parallel
    pub fn handle_client_requests(self, mut connection: TcpStream, cache: Cache<usize, Vec<u8>>) {
        let (req_sender, req_receiver) = sync_channel::<Option<ClientRequest>>(1000);
        let (resp_sender, resp_receiver) = sync_channel::<ClientResponse>(1000);
        let resp_sender_clone = resp_sender.clone();
        
        let mut reader = BufReader::new(connection.try_clone().unwrap());
        let cache_writer = cache.clone();
        let self_state = Arc::new(Mutex::new(self));

        let producer = thread::spawn(move || {
            Proxy::get_client_requests(&mut reader, cache, req_sender, resp_sender_clone);
        });

        let writer = thread::spawn(move || {
            while let Ok(resp) = resp_receiver.recv() {
                trace!("Sending response to client: id={}, key={}, type={:?}", 
                      resp.request_id, resp.key, resp.request_type);
                
                let resp_bytes = resp.to_bytes();
                connection.write_all(&resp_bytes).unwrap();
                if resp.request_type == READ {
                    cache_writer.insert(resp.key as usize, resp.value);
                }
            }
        });

        let consumer = thread::spawn(move || {
            let mut self_state = self_state.lock().unwrap();
            match self_state.config.proxy.is_unsafe {
                true => self_state.consume_client_requests_unsafe(req_receiver, resp_sender),
                false => self_state.consume_client_requests(req_receiver, resp_sender)
            }
        });

        producer.join().unwrap();
        info!("Producer joined");
        consumer.join().unwrap();
        info!("Consumer joined");
        writer.join().unwrap();
        info!("Writer joined");
    }

    /// Request processing loop for the non-oblivious ("unsafe") baseline, enabled with
    /// `is_unsafe = true`. Requests are batched on the same time interval and go through
    /// the same cache and encryption, but each batch reads/writes exactly the requested
    /// locations: no dummy accesses, no reshuffling, and the position map never changes.
    pub fn consume_client_requests_unsafe(&mut self, req_receiver: Receiver<Option<ClientRequest>>, resp_sender: SyncSender<ClientResponse>) {
        println!("Starting consumer (unsafe, non-oblivious baseline)");
        let mut req_map: HashMap<usize, Vec<ClientRequest>> = HashMap::new();
        let batch_interval = Duration::from_millis(self.config.proxy.batch_interval_ms);
        let mut last_batch_time = Instant::now();

        loop {
            // Use recv_timeout to allow periodic time checks
            let recv_result = req_receiver.recv_timeout(batch_interval);

            match recv_result {
                Ok(Some(req)) => {
                    if req_map.contains_key(&(req.key as usize)) {
                        match req.request_type {
                            READ => {
                                let reqs = req_map.get_mut(&(req.key as usize)).unwrap();
                                // if the request is a read and the key is in the map, and the only request is a write, then send the value of the write to the client
                                if reqs.len() == 1 && reqs[0].request_type == WRITE {
                                    trace!("Found pending WRITE for READ request: id={}, key={}", req.request_id, req.key);
                                    let resp = ClientResponse{
                                        request_id: req.request_id,
                                        request_type: READ,
                                        key: req.key,
                                        value: reqs[0].value.clone()
                                    };
                                    resp_sender.send(resp).unwrap();
                                } else {
                                    trace!("Adding READ request to request map: id={}, key={}", req.request_id, req.key);
                                    req_map.get_mut(&(req.key as usize)).unwrap().push(req);
                                }
                                continue;
                            },
                            WRITE => {
                                trace!("Processing pending requests with new WRITE: id={}", req.request_id);
                                for req_read in req_map.get_mut(&(req.key as usize)).unwrap() {
                                    if req_read.request_type == READ {
                                        trace!("Sending READ response to client because of new WRITE: id={}, key={}", req_read.request_id, req_read.key);
                                        let resp = ClientResponse{
                                            request_id: req_read.request_id,
                                            request_type: READ,
                                            key: req_read.key,
                                            value: req.value.clone()
                                        };
                                        resp_sender.send(resp).unwrap();
                                    }
                                }
                            }
                        }
                    }

                    req_map.insert(req.key as usize, vec![req]);
                },
                Ok(None) => {
                    // Termination signal received
                    break;
                },
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Timeout - check if we should process a batch
                },
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // Channel disconnected
                    break;
                }
            }

            // Time-based batch trigger
            let is_batch_ready = last_batch_time.elapsed() >= batch_interval && !req_map.is_empty();

            if is_batch_ready {
                let mut batch_read: Vec<usize> = Vec::new();
                let mut batch_write: Vec<(usize, Vec<u8>)> = Vec::new();

                for (key, reqs) in req_map.iter() {
                    match reqs[0].request_type {
                        READ => batch_read.push(*key),
                        WRITE => batch_write.push((*key, reqs[0].value.clone())),
                    }
                }
 
                let batch_read_locations: Vec<usize> = batch_read.iter().map(|x| self.pmap.forward[*x]).collect();
                let batch_write_locations: Vec<usize> = batch_write.iter().map(|x| self.pmap.forward[x.0]).collect();
                let mut batch_write_values: Array2<u8> = Array2::zeros((batch_write.len(), self.config.common.object_size));
                for i in 0..batch_write.len() {
                    let value = &batch_write[i].1;
                    let mut value_slice = batch_write_values.slice_mut(s![i, ..]);
                    value_slice.as_slice_mut().unwrap().copy_from_slice(value);
                }

                if !batch_write.is_empty() {
                    let mut batch_write_encrypted_values = Array2::<u8>::zeros((batch_write.len(), self.encrypted_size));
                    let chunk_size = (batch_write.len()/self.config.proxy.num_threads)+1;
                    thread::scope(|s| {
                        for (plain_slice, mut target_slice) in 
                        batch_write_values.axis_chunks_iter(Axis(0), chunk_size).zip(batch_write_encrypted_values.axis_chunks_iter_mut(Axis(0), chunk_size)) {
                            s.spawn(move || {
                                let encryptor = Encryptor::new();
                                for i in 0..plain_slice.shape()[0] {
                                    encryptor.encrypt_object_in(
                                        plain_slice.slice(s![i, ..]).as_slice().unwrap(), 
                                        target_slice.slice_mut(s![i, ..]).as_slice_mut().unwrap());
                                }
                            });
                        }
                    });

                    self.write_request(&batch_write_locations, &batch_write_encrypted_values);
                }

                if !batch_read.is_empty() {
                    let storage_resp_arr = self.get_request(&batch_read_locations, false);
                    let mut storage_resp_arr_plain = Array2::<u8>::zeros((batch_read.len(), self.config.common.object_size));
                    let chunk_size = (batch_read.len()/self.config.proxy.num_threads)+1;
                    thread::scope(|s| {
                        for (mut plain_slice, encrypted_slice) in 
                        storage_resp_arr_plain.axis_chunks_iter_mut(Axis(0), chunk_size).zip(storage_resp_arr.axis_chunks_iter(Axis(0), chunk_size)) {
                            s.spawn(move || {
                                let encryptor = Encryptor::new();
                                for i in 0..plain_slice.shape()[0] {
                                    encryptor.decrypt_object_in(
                                        encrypted_slice.slice(s![i, ..]).as_slice().unwrap(), 
                                        plain_slice.slice_mut(s![i, ..]).as_slice_mut().unwrap());
                                }
                            });
                        }
                    });

                    for i in 0..batch_read.len() {
                        let resp_value = storage_resp_arr_plain.slice(s![i, ..]).as_slice().unwrap().to_vec();
                        match req_map.get(&batch_read[i]) {
                            Some(client_reqs) => {
                                client_reqs.iter().for_each(|client_req| {
                                    if client_req.request_type == READ {
                                        trace!("Sending READ response from batch: id={}, key={}", client_req.request_id, client_req.key);
                                        let resp = ClientResponse{
                                            request_id: client_req.request_id, 
                                            request_type: READ, 
                                            key: client_req.key, 
                                            value: resp_value.clone()
                                        };
                                        resp_sender.send(resp).unwrap();
                                    }
                                });
                                req_map.remove(&batch_read[i]);
                            },
                            None => {
                                trace!("No pending requests for key={} in batch", batch_read[i]);
                            }
                        }
                    }
                }

                // Clear after every flush, not only when the batch had reads:
                // write-only batches must not leave their keys behind, or every
                // later flush re-writes the whole cumulative write-set.
                req_map.clear();

                last_batch_time = Instant::now();
            }
        }
    }

    /// Request processing loop that implements the ORAM protocol logic
    /// - Groups requests into batches based on time-set budgets
    /// - Performs dummy accesses to maintain fixed access patterns
    /// - Shuffles real and dummy requests before accessing storage
    pub fn consume_client_requests(&mut self, req_receiver: Receiver<Option<ClientRequest>>, resp_sender: SyncSender<ClientResponse>) {
        println!("Starting consumer (oblivious)");
        let mut cur_timestamp = self.time_sets.budgets.len() - 1;
        let mut req_map: HashMap<usize, Vec<ClientRequest>> = HashMap::new();
        let t = self.time_sets.budgets.len();
        let mut rng = SmallRng::from_rng(&mut rng());
        let batch_interval = Duration::from_millis(self.config.proxy.batch_interval_ms);
        let mut last_batch_time = Instant::now();
        let batch_size = self.time_sets.budgets.iter().sum::<usize>();

        let mut budget_log_writer: Option<File> = if self.config.proxy.enable_budget_log {
            let mut writer = File::create(&self.config.proxy.budget_log_file).unwrap();
            let budgets_str = self.time_sets.budgets.iter()
                .map(|b| b.to_string())
                .collect::<Vec<String>>()
                .join(",");
            writeln!(writer, "{}", budgets_str).unwrap();
            Some(writer)
        } else {
            None
        };

        loop {
            // Use recv_timeout to allow periodic time checks
            let recv_result = req_receiver.recv_timeout(batch_interval);

            match recv_result {
                Ok(Some(req)) => {
                    trace!("Processing request in consumer: id={}, key={}, type={:?}",
                          req.request_id, req.key, req.request_type);

                    if req_map.contains_key(&(req.key as usize)) {
                        match req.request_type {
                            READ => {
                                let reqs = req_map.get_mut(&(req.key as usize)).unwrap();
                                // if the request is a read and the key is in the map, and the only request is a write, then send the value of the write to the client
                                if reqs.len() == 1 && reqs[0].request_type == WRITE {
                                    trace!("Found pending WRITE for READ request: id={}, key={}", req.request_id, req.key);
                                    let resp = ClientResponse{
                                        request_id: req.request_id,
                                        request_type: READ,
                                        key: req.key,
                                        value: reqs[0].value.clone()
                                    };
                                    resp_sender.send(resp).unwrap();
                                } else {
                                    trace!("Adding READ request to request map: id={}, key={}", req.request_id, req.key);
                                    req_map.get_mut(&(req.key as usize)).unwrap().push(req);
                                }
                                continue;
                            },
                            WRITE => {
                                trace!("Processing pending requests with new WRITE: id={}", req.request_id);
                                for req_read in req_map.get_mut(&(req.key as usize)).unwrap() {
                                    if req_read.request_type == READ {
                                        trace!("Sending READ response to client because of new WRITE: id={}, key={}", req_read.request_id, req_read.key);
                                        let resp = ClientResponse{
                                            request_id: req_read.request_id,
                                            request_type: READ,
                                            key: req_read.key,
                                            value: req.value.clone()
                                        };
                                        resp_sender.send(resp).unwrap();
                                    }
                                }
                            }
                        }
                    }
                    let reqs_time_set = cur_timestamp - self.pmap.time_stamps_observed[req.key as usize];
                    self.time_sets.insert(req.key as usize, reqs_time_set, req.request_type);
                    req_map.insert(req.key as usize, vec![req]);
                },
                Ok(None) => {
                    // Termination signal received
                    break;
                },
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Timeout - check if we should process a batch
                },
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // Channel disconnected
                    break;
                }
            }

            // Time-based batch trigger
            let mut is_batch_ready = last_batch_time.elapsed() >= batch_interval && !req_map.is_empty();

            if !is_batch_ready && (req_map.len() > self.config.proxy.queue_coefficient * batch_size) {
                //sleep until next batch time
                let time_to_sleep = batch_interval.checked_sub(last_batch_time.elapsed()).unwrap_or(Duration::from_secs(0));
                thread::sleep(time_to_sleep);
                is_batch_ready = true;
            }

            if is_batch_ready {
                let mut batch_keys: Vec<(usize, RequestType)> = Vec::new();
                let mut real_batch_keys: HashSet<usize> = HashSet::new();

                let mut used_budget = Vec::new();
                for i in 0..t {
                    let (time_set, queue, budget) = self.time_sets.index_mut(i);
                    let mut req_counter = 0;

                    while !queue.is_empty() && req_counter < budget {
                        req_counter += 1;
                        let (key, request_type) = queue.pop_front().unwrap();
                        time_set.delete_by_value(&key);
                        batch_keys.push((key, request_type));
                        real_batch_keys.insert(key);
                    }

                    used_budget.push(req_counter);

                    if budget > req_counter {
                        for dummy_req in time_set.pop_first_x_max(budget - req_counter) {
                            batch_keys.push((dummy_req.1, READ));
                        }
                    }
                }

                if let Some(ref mut writer) = budget_log_writer {
                    let used_budget_str = used_budget.iter()
                        .map(|b| b.to_string())
                        .collect::<Vec<String>>()
                        .join(",");
                    writeln!(writer, "{}", used_budget_str).unwrap();
                    writer.flush().unwrap();
                }
                
                batch_keys.shuffle(&mut rng);
                let batch_locations = batch_keys.iter().map(|x| self.pmap.forward[x.0]).collect();
                let write_indices_and_values = batch_keys
                    .iter()
                    .enumerate()
                    .filter(|x| x.1.1 == WRITE)
                    .map(|x| (x.0, req_map.get(&x.1.0).unwrap()[0].value.clone()))
                    .collect::<Vec<(usize, Vec<u8>)>>();

                let mut permutation_arr: Vec<usize> = (0..batch_keys.len()).collect::<Vec<usize>>();
                permutation_arr.shuffle(&mut rng);
                
                let server_resp = self.storage_read_write(&batch_locations, &permutation_arr, &write_indices_and_values);
                for i in 0..batch_keys.len() {
                    let resp_value = server_resp.slice(s![i, ..]).as_slice().unwrap().to_vec();
                    match req_map.get(&batch_keys[i].0) {
                        Some(client_reqs) => {
                            client_reqs.iter().for_each(|client_req| {
                                if client_req.request_type == READ {
                                    trace!("Sending READ response from batch: id={}, key={}", client_req.request_id, client_req.key);
                                    let resp = ClientResponse{
                                        request_id: client_req.request_id, 
                                        request_type: READ, 
                                        key: client_req.key, 
                                        value: resp_value.clone()
                                    };
                                    resp_sender.send(resp).unwrap();
                                }
                            });
                            req_map.remove(&batch_keys[i].0);
                        },
                        None => {
                            trace!("No pending requests for key={} in batch", batch_keys[i].0);
                        }
                    }
                }

                let next_timestamp = cur_timestamp + 1;
                for i in 0..permutation_arr.len() {
                    self.pmap.set_key(batch_keys[permutation_arr[i]].0, batch_locations[i], next_timestamp, real_batch_keys.contains(&batch_keys[permutation_arr[i]].0));
                }
                let mut batch_set = HeapPQ::<usize, usize>::new();
                for x in &batch_keys {
                    batch_set.insert(self.pmap.time_stamps_real[x.0], x.0);
                }
                self.time_sets.add_time_set(batch_set);
                cur_timestamp = next_timestamp;

                last_batch_time = Instant::now();
            }
        }
        info!("Consumer finished processing requests");
        if let Some(ref mut writer) = budget_log_writer {
            writer.flush().unwrap();
        }
    }

    /// Initializes the storage server with encrypted dummy data
    pub fn storage_init(&self) {
        info!("Initializing storage server with encrypted dummy data");
        let mut writer = BufWriter::new(self.connection.try_clone().unwrap());
        writer.write_all(&(self.calc_storage_size as u32).to_ne_bytes()).unwrap();
        writer.write_all(&(self.config.common.object_size as u32).to_ne_bytes()).unwrap();
        writer.flush().unwrap();

        info!("sent storage init header");

        let encryptor = Encryptor::new();

        info!("initialized encryptor");

        self.get_request(&(0..self.calc_storage_size).collect(), true);

        info!("requested everything in storage server");
        let mut storage = Array2::<u8>::zeros((self.calc_storage_size, self.encrypted_size));
        (0..self.calc_storage_size).for_each(|x| encryptor.encrypt_object_in(&vec![(x % 255) as u8; self.config.common.object_size], storage.slice_mut(s![x, ..]).as_slice_mut().unwrap() ));

        info!("prepared dummy storage data");
        self.write_back(&storage);
        info!("Storage initialization complete");
    }

    /// Sends a read request to storage server for multiple locations
    fn get_request(&self, storage_req_arr: &Vec<usize>, does_write_back: bool) -> Array2<u8> {
        let mut writer = BufWriter::new(self.connection.try_clone().unwrap());
        
        info!("sending get request, write_back: {}", does_write_back);
        match does_write_back {
            true => writer.write_all(&[0u8]).unwrap(),
            false => writer.write_all(&[1u8]).unwrap(),
        }

        info!("sent request type");
        let len_bytes = (storage_req_arr.len() as u32).to_ne_bytes();
        info!("sending request length bytes: {:x?}", len_bytes);
        writer.write_all(&len_bytes).unwrap();
        writer.flush().unwrap();
        info!("sent request length: {}", storage_req_arr.len());
        for i in 0..storage_req_arr.len() {
            writer.write_all(&(storage_req_arr[i] as u32).to_ne_bytes()).unwrap();
        }
        writer.flush().unwrap();
        info!("sent request indices");

        let mut reader = BufReader::new(self.connection.try_clone().unwrap());
        let mut storage_resp_arr = Array2::<u8>::zeros((storage_req_arr.len(), self.encrypted_size));
        
        let objects = storage_resp_arr.as_slice_mut().unwrap();
        reader.read_exact(objects).unwrap();

        info!("received response from storage");

        match does_write_back {
            true => {},
            false => {
                let mut buf = [0u8; 1];
                reader.read_exact(&mut buf).unwrap();
            },
        }
        storage_resp_arr
    }

    /// Writes back encrypted objects to storage server after shuffling
    fn write_back(&self, permuted_storage_resp_arr: &Array2::<u8>) {
        let mut writer = BufWriter::new(self.connection.try_clone().unwrap());  
        writer.write_all(permuted_storage_resp_arr.as_slice().unwrap()).unwrap();
        writer.flush().unwrap();
        let mut reader = BufReader::new(self.connection.try_clone().unwrap());
        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf).unwrap();
    }

    /// Writes encrypted objects to the given storage locations (used by the unsafe baseline)
    fn write_request(&self, index_arr: &Vec<usize>, value_arr: &Array2::<u8>) {
        let mut writer = BufWriter::new(self.connection.try_clone().unwrap());
        writer.write_all(&[2u8]).unwrap();
        writer.write_all(&(index_arr.len() as u32).to_ne_bytes()).unwrap();
        for i in 0..index_arr.len() {
            writer.write_all(&(index_arr[i] as u32).to_ne_bytes()).unwrap();
        }
        writer.write_all(value_arr.as_slice().unwrap()).unwrap();
        writer.flush().unwrap();
        let mut reader = BufReader::new(self.connection.try_clone().unwrap());
        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf).unwrap();
    }

    /// Performs a batch of read operations from storage followed by
    /// a shuffled write-back to hide access patterns
    /// Returns the decrypted objects for real requests
    fn storage_read_write(&mut self, storage_req_arr: &Vec<usize>, permutation_arr: &Vec<usize>, write_indices_and_values: &Vec<(usize, Vec<u8>)>) -> Array2::<u8> {
        let storage_resp_arr = self.get_request(storage_req_arr, true);

        let mut storage_resp_arr_plain = Array2::<u8>::zeros((storage_req_arr.len(), self.config.common.object_size));
        let chunk_size = (storage_req_arr.len()/self.config.proxy.num_threads)+1;
        thread::scope(|s| {
            for (mut plain_slice, encrypted_slice) in 
            storage_resp_arr_plain.axis_chunks_iter_mut(Axis(0), chunk_size).zip(storage_resp_arr.axis_chunks_iter(Axis(0), chunk_size)) {
                s.spawn(move || {
                    let encryptor = Encryptor::new();
                    for i in 0..plain_slice.shape()[0] {
                        encryptor.decrypt_object_in(
                            encrypted_slice.slice(s![i, ..]).as_slice().unwrap(), 
                            plain_slice.slice_mut(s![i, ..]).as_slice_mut().unwrap());
                    }
                });
            }
        });

        let mut permuted_storage_resp_arr_plain = Array2::<u8>::zeros((storage_req_arr.len(), self.config.common.object_size));
        for i in 0..permutation_arr.len() {
            permuted_storage_resp_arr_plain.slice_mut(s![i, ..]).assign(&storage_resp_arr_plain.slice(s![permutation_arr[i], ..]));
        }

        // write the WRITE requests values to the permuted storage response array
        for (i, value) in write_indices_and_values {
            let value_arr = Array2::from_shape_vec((1, value.len()), value.clone()).unwrap();
            permuted_storage_resp_arr_plain.slice_mut(s![*i, ..value.len()]).assign(&value_arr.slice(s![0, ..]));
        }

        let mut permuted_storage_resp_arr = Array2::<u8>::zeros((storage_req_arr.len(), self.encrypted_size));
        thread::scope(|s| {
            for (plain_slice, mut target_slice) in 
            permuted_storage_resp_arr_plain.axis_chunks_iter(Axis(0), chunk_size).zip(permuted_storage_resp_arr.axis_chunks_iter_mut(Axis(0), chunk_size)) {
                s.spawn(move || {
                    let encryptor = Encryptor::new();
                    for i in 0..plain_slice.shape()[0] {
                        encryptor.encrypt_object_in(
                            plain_slice.slice(s![i, ..]).as_slice().unwrap(), 
                            target_slice.slice_mut(s![i, ..]).as_slice_mut().unwrap());
                    }
                });
            }
        });

        self.write_back(&permuted_storage_resp_arr);

        storage_resp_arr_plain
    }
}

