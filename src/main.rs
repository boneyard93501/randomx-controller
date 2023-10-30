#![feature(lazy_cell)]
#![feature(file_create_new)]
use cfg_handler::RandomxCfg;
use cfg_handler::RuntimeCfg;
use chrono::{Local, Utc};
use crossbeam::channel::{unbounded, Receiver, Sender};
use fluence_keypair::KeyPair;
use log::*;
use std::fs::File;
use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
    Arc, LazyLock, Mutex, RwLock,
};
use std::thread;
use std::time::Duration;


mod cfg_handler;
mod hashers;
mod keyblock;
mod mocks;
mod pow;
mod puzzle;
mod pid_handler;


const LOG_PATH: &str = "./logs/log.txt";
const PID_PATH: &str = "./pid.json";
const SETUP_CFG_PATH: &str = "./data/randomx_cfg.json";
const RUNTME_CFG_PATH: &str = "./data/runtime_cfg.json";
const PUZZLE_SOLUTION_DIR: &str = "./puzzle-solutions/";
const KEYBLOCK_CHECK_INTERVAL: u32 = 30 * 60 * 1000; // in millis
const MAIN_LOOP_SLEEP: u32 = 6 * 1_000; // in millis
const BLOCK_KEY_OFFSET: u32 = 2_048;
const BLOCK_KEY_DELAY: u32 = 64;

static MAX_THREAD_COUNT: AtomicU32 = AtomicU32::new(0);
static ALLOC_THREAD_COUNT: AtomicU32 = AtomicU32::new(0);

static CURRENT_KEYBLOCK: AtomicU64 = AtomicU64::new(1);

static PUZZLE_DIFFICULTY:AtomicU32 = AtomicU32::new(100);

static APP_EXIT: AtomicBool = AtomicBool::new(false);
static RANDOMX_RESTART: AtomicBool = AtomicBool::new(false);

static KEYPAIR: LazyLock<Arc<KeyPair>> = LazyLock::new(|| Arc::new(KeyPair::generate_ed25519()));
static PEERID: LazyLock<Arc<String>> =
    LazyLock::new(|| Arc::new(KEYPAIR.get_peer_id().to_base58()));

fn golden_hash_processor() -> thread::JoinHandle<()> {
    let thread_handle = thread::spawn(move || {
        let mut last_rpc_call = Utc::now().timestamp_millis();

        loop {
            // check for keyblock updates every x seconds
            if Utc::now().timestamp_millis() - last_rpc_call > (KEYBLOCK_CHECK_INTERVAL as i64) {
                // run key block updater
                last_rpc_call = Utc::now().timestamp_millis();
            }

            // check for exit updates every x seconds

            // check for allocation changes
        }
    });
    thread_handle
}

fn setup_logging() {
    let file = match File::create_new(LOG_PATH) {
        Ok(f) => f,
        Err(_) => File::options().append(true).open(LOG_PATH).unwrap(),
    };
    let target = Box::new(file);

    env_logger::Builder::new()
        .target(env_logger::Target::Pipe(target))
        .filter(None, LevelFilter::Debug)
        .format(|buf, record| {
            writeln!(
                buf,
                "[{} {} {}:{}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.args()
            )
        })
        .init();
}

fn global_config_setter(app_cfg: &RandomxCfg, runtime_cfg: &RuntimeCfg) -> Result<(), ()> {
    let t_max_alloc = app_cfg.num_cores * app_cfg.threads_per_core;
    MAX_THREAD_COUNT.swap(t_max_alloc, Relaxed);

    let t_dealloc = runtime_cfg.deallocated_threads;
    if t_dealloc < MAX_THREAD_COUNT.load(Relaxed) {
        let actual_alloc = MAX_THREAD_COUNT.load(Relaxed) - t_dealloc;
        ALLOC_THREAD_COUNT.swap(actual_alloc, Relaxed);
    } else {
        log::error!("invalid thread de-allocation: {}. dealloc request ignored. use ctrl-c to shut down app.", t_dealloc);
    }

    PUZZLE_DIFFICULTY.swap(app_cfg.difficulty, Relaxed);
    Ok(())
}

fn main() {

    // handle pid file
    pid_handler::rm_pid();
    pid_handler::write_pid();
    
    setup_logging();

    //ctrlc and limited sigterm catcher
    let (crlc_tx, crlc_rx) = unbounded();
    ctrlc::set_handler(move || crlc_tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");
    log::info!("crlc channel is up.");


    // get and set randomx config
    let app_cfg = cfg_handler::RandomxCfg::from_file().unwrap();
    let runtime_cfg = cfg_handler::RuntimeCfg::from_file().unwrap();
    global_config_setter(&app_cfg, &runtime_cfg).unwrap();
    log::info!("global config updated.");

    // get and set keyblock
    let (_, _) = keyblock::keyblock_handler(&app_cfg.key_blockchain_uri).unwrap();


    // randomx channel to communicate puzzle solution for further processing such as proof generation
    let (tx, rx): (
        Sender<puzzle::PuzzleSolution>,
        Receiver<puzzle::PuzzleSolution>,
    ) = unbounded();
    log::info!("RandomX channel is up.");


    // setup and fire up the threaded randomx instances
    let alloc_threads: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::<String>::new()));
    let dealloc_threads: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::<String>::new()));
    let dealloc_requests: Arc<RwLock<u32>> = Arc::new(RwLock::<u32>::new(0));
    let randomx_up_counter: Arc<RwLock<u32>> = Arc::new(RwLock::<u32>::new(0));

    let mut thread_handler:Vec<thread::JoinHandle<()>>;

    // start initiating threads
    thread_handler = pow::randomx_thread_pool_handler( 
        MAX_THREAD_COUNT.load(Relaxed),
        CURRENT_KEYBLOCK.load(Relaxed),
        tx.clone(),
        &alloc_threads, 
        &dealloc_threads, 
        &dealloc_requests,
        &randomx_up_counter,
        None
    ).unwrap();
   
    log::info!("setup done.");

    log::info!("waiting for randomx disk to be initiated for each instance. This takes a while.");
    loop {
        match randomx_up_counter.read() {
            Ok(r) => {
                if *r == ALLOC_THREAD_COUNT.load(Relaxed) {
                    break;
                }
            },
            Err(_) => {thread::sleep(Duration::from_millis(5_000));}
        }
    }
    log::info!("{} randomx disks are initiated.", *randomx_up_counter.read().unwrap());
    
    // set key block getting time -- good enough
    let mut last_rpc_call = Utc::now().timestamp_millis();

    //main monitoring loop -- trying to preserve threads for randomx
    log::info!("entering main control loop.");
    loop {
        // check for key block updates every x seconds
        if Utc::now().timestamp_millis() - last_rpc_call > (KEYBLOCK_CHECK_INTERVAL as i64) {
            // run key block updater
            let (key_block, updated_kb) = keyblock::keyblock_handler(&app_cfg.key_blockchain_uri).unwrap();
            // we got a new keyblock and need to tear down the randomx instances and initate with new disks
            if updated_kb {
                log::info!("got a new key block {} and need to restart randomx threads.", key_block);
                RANDOMX_RESTART.swap(true, Relaxed);
                for t in &thread_handler {
                    t.is_finished();
                }
                RANDOMX_RESTART.swap(true, Relaxed);

                let guard = randomx_up_counter.write();
                if guard.is_ok() {
                    let mut rw_guard = guard.unwrap();
                    *rw_guard = 0;
                }

                thread_handler = pow::randomx_thread_pool_handler( 
                    alloc_threads.lock().unwrap().len() as u32,
                    CURRENT_KEYBLOCK.load(Relaxed),
                    tx.clone(),
                    &alloc_threads, 
                    &dealloc_threads, 
                    &dealloc_requests,
                    &randomx_up_counter,
                    None
                ).unwrap();
                
                log::info!("waiting for randomx disk to be initiated for each instance. This takes a while.");
                loop {
                    match randomx_up_counter.read() {
                        Ok(r) => {
                            if *r == alloc_threads.lock().unwrap().len() as u32 {
                                break;
                            }
                        },
                        Err(_) => {thread::sleep(Duration::from_millis(5_000));}
                    }
                }
                log::info!("{} randomx disks are initiated.", *randomx_up_counter.read().unwrap());
            }
            log::info!("keyblock update check");
            last_rpc_call = Utc::now().timestamp_millis();
        }

        // check for allocation changes
        let run_cfg = cfg_handler::RuntimeCfg::from_file().unwrap();
        let dealloc_count = dealloc_threads.lock().unwrap().len() as u32;
        if run_cfg.deallocated_threads > dealloc_count as u32 {
            if MAX_THREAD_COUNT.load(Relaxed).checked_sub(run_cfg.deallocated_threads).is_none() {
                log::warn!("invalid thread reduction request for minus {} threads ignored.", run_cfg.deallocated_threads);
            }
            else {
                let thread_decr = run_cfg.deallocated_threads - dealloc_count;
                log::info!("need to reduce thread count to {} by {} threads", dealloc_count, thread_decr);
                let dealloc = dealloc_requests.write();
                let mut rw_guard = dealloc.unwrap();
                *rw_guard = thread_decr;
            } // drop dealloc lock
            thread::sleep(Duration::from_millis(3_000));
            let new_alloc_count = alloc_threads.lock().unwrap().len() as u32;
            let new_dealloc_count = dealloc_threads.lock().unwrap().len() as u32;
            log::info!("update allocated thread count: {} and deallocated thread count: {}", new_alloc_count,new_dealloc_count);
        }
        // note we can't realloc more than we deallocated since n_alloc + n_dealloc === MAX_THREADS 
        else if run_cfg.deallocated_threads < dealloc_count as u32 {
            let alloc_delta = dealloc_count - run_cfg.deallocated_threads;
            log::info!("need to increase thread count by {} threads", alloc_delta);
            let delta_thread_names = dealloc_threads.lock().unwrap().clone();
            let delta_thread_names:Vec<&str> = delta_thread_names.iter().map(|s| s.as_str()).collect();
            let delta_thread_names = delta_thread_names[0..alloc_delta as usize].to_vec();
            let new_thread_handles = pow::randomx_thread_pool_handler(
            delta_thread_names.len() as u32,
            CURRENT_KEYBLOCK.load(Relaxed),
                tx.clone(),
                &alloc_threads, 
                &dealloc_threads, 
                &dealloc_requests,
                &randomx_up_counter,
                Some(delta_thread_names.clone()),
            )
            .unwrap();
            log::info!("waiting for additional randomx disks to be initiated. This takes a while.");
            // thread::sleep(Duration::from_millis(7_000));
            loop {
                match randomx_up_counter.read() {
                    Ok(r) => {
                        if *r == alloc_threads.lock().unwrap().len() as u32 {
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
                    },
                    Err(_) => {thread::sleep(Duration::from_millis(2_000));}
                }
            }
            log::info!("A total of {} randomx disks are initiated.", *randomx_up_counter.read().unwrap());

            // clean up dealloc references
            log::info!("starting cleanup of dealloc tracker.");    
            let mut dealloc_guard = dealloc_threads.lock().unwrap();
            for reg_name in delta_thread_names {
                let v = &mut dealloc_guard;
                v.retain(|name| name != &reg_name.to_string());
            }
            drop(dealloc_guard);
            thread_handler.extend(new_thread_handles);
        }
        println!("alloc vec       : {:?}", alloc_threads.lock().unwrap());
        println!("dealloc vec     : {:?}", dealloc_threads.lock().unwrap());
        println!("dealloc requests: {:?}", dealloc_requests.read().unwrap());
        println!("up counter      : {}", *randomx_up_counter.read().unwrap());

        // process puzzle solutions -- depending on success frequency this could be another thread
        loop {
            if !rx.is_empty() {
                let solution = rx.recv().unwrap();
                // verify just for the heck of it .. breadcrumps for mike :)
                // let good_solution = pow::randomx_verifier(&solution.signed_context, &solution.signed_nonce, &solution.difficulty, &solution.hash);
                match solution.to_file(None) {
                    Ok(_) => {},
                    Err(e) => { println!("{}", e);}
                }
            } 
            else {
                break;
            }
        }

        // check for sigterm        
        if !crlc_rx.is_empty() {
            match crlc_rx.recv() {
                Ok(_) => {
                    log::info!("received sigterm signal ... shutting down.");
                    println!("received sigterm signal and initiated shut down. This takes a minute ... patience.");
                    APP_EXIT.swap(true, Relaxed);
                    break;
                }
                Err(_) => { println!("no crtlc in channel"); }
            }
        }
        thread::sleep(Duration::from_millis(MAIN_LOOP_SLEEP as u64));
    }

    // teardown -- might be blocking for SIGTERM
    let timer_start = Utc::now().timestamp_millis();
    let max_shutdown_duration: i32 = 15 * 1_000; // 15 seconds to clean things up

    while Utc::now().timestamp_millis() - timer_start < (max_shutdown_duration as i64) {
        // all threads exited ? log::info!("threads have exited")

        // all channels empty ? log::info!("channels are empty")

        thread::sleep(Duration::from_millis(100));
    }
    log::info!("done with interrupt catcher.");
    // finally join 
    for t in thread_handler {
        t.join().unwrap();
    }
    log::info!("done and done. exiting main.");
}
