use chrono::Utc;
use crossbeam::channel::Sender;
use rust_randomx::{ Context, Hasher };
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::sync::atomic::Ordering::Relaxed;

use crate::hashers;
use crate::mocks;
use crate::puzzle;
use crate::{APP_EXIT, RANDOMX_RESTART};

use crate::{PEERID, PUZZLE_DIFFICULTY};

type AMVS = Arc<Mutex<Vec<String>>>;
type ARU32= Arc<RwLock<u32>>;

pub fn randomx_fast_instance(
    key_block: &u64,
    peer_id: &str,
    sender: &Sender<puzzle::PuzzleSolution>,
    puzzle_difficulty: &u32,
    alloc_threads:AMVS,
    dealloc_threads:AMVS,
    dealloc_requests:ARU32,
    randomx_up_counter:ARU32
) {
    let context_raw = format!("{}{}", key_block, &thread::current().name().unwrap());
    let context_hash = hashers::keccak_hasher(&context_raw);
    let signed_context = mocks::signer(&context_hash.to_vec());
    let context = Arc::new(Context::new(&signed_context, true));

    // update "up" counter
    let guard = randomx_up_counter.write();
    if guard.is_ok() {
        let mut rw_guard = guard.unwrap();
        *rw_guard += 1;
    }

    log::info!("hasher setup {}", thread::current().name().unwrap());

    let mut randomx_hasher = Hasher::new(context);
    let nonce_raw: u64 = (Utc::now().timestamp_millis() as u64) + key_block;
    let mut nonce = mocks::signer(&nonce_raw.to_le_bytes().to_vec());

    randomx_hasher.hash_first(&nonce);
    loop {
        let next_nonce_raw: u64 = (Utc::now().timestamp_millis() as u64) + key_block;
        let next_nonce = mocks::signer(&next_nonce_raw.to_le_bytes().to_vec());
        let out = randomx_hasher.hash_next(&next_nonce);
        
        if out.leading_zeros() == *puzzle_difficulty {
            let solution = puzzle::PuzzleSolution::new(
                peer_id.clone().as_bytes().to_vec(),
                key_block.clone(),
                signed_context.to_vec(),
                thread::current().name().unwrap().as_bytes().to_vec(),
                nonce_raw.to_le_bytes().to_vec(),
                nonce.to_vec(), //signed nonce
                out.as_ref().to_vec(),
                puzzle_difficulty.clone(),
            );
            sender.send(solution).unwrap();
            // log::info!("got a match {}", thread::current().name().unwrap());
        }
        nonce = next_nonce;

        if thread_dealloc(&alloc_threads, &dealloc_threads, &dealloc_requests, &randomx_up_counter) {
            log::info!("dealloc exit for thread {}", thread::current().name().unwrap());
            break;
        }

        if RANDOMX_RESTART.load(Relaxed) {
            log::info!("app exit for thread {}", thread::current().name().unwrap());
            break;
        }

        if APP_EXIT.load(Relaxed) {
            log::info!("app exit for thread {}", thread::current().name().unwrap());
            break;
        }
    }
}

fn thread_dealloc(alloc_threads:&AMVS, dealloc_threads:&AMVS, dealloc_requests:&ARU32, randomx_up_counter: &ARU32) -> bool {
    let mut dealloc_exit = false;

    let reader = dealloc_requests.read();
    if reader.is_ok() && *reader.unwrap() > 0 {
        // lock counter
        let c_lock = dealloc_requests.write();
        if c_lock.is_ok() {
            let mut rw_guard = c_lock.unwrap();
            *rw_guard -= 1;
        } // drop counter lock
            
        let reg_name = format!("{}", thread::current().name().unwrap());
        
        let mut alloc_guard = alloc_threads.lock().unwrap();
        let v = &mut alloc_guard;
        v.retain(|name| name != &reg_name);
        drop(alloc_guard); //manual drop to speed things up

        let mut dealloc_guard = dealloc_threads.lock().unwrap();
        let v = &mut dealloc_guard;
        v.push(reg_name);
        drop(dealloc_guard);

        let up_counter_guard = randomx_up_counter.write();
        if up_counter_guard.is_ok() {
            let mut rw_guard = up_counter_guard.unwrap();
            *rw_guard -= 1;
        }
        
        dealloc_exit = true
    }
    dealloc_exit
}

pub fn randomx_thread_pool_handler(
    num_threads: u32,
    key_block: u64,
    tx: Sender<puzzle::PuzzleSolution>,
    alloc_threads:&AMVS, 
    dealloc_threads:&AMVS, 
    dealloc_requests:&ARU32,
    randomx_up_counter:&ARU32,
    reg_names: Option<Vec<&str>>
    ) -> Result<Vec<thread::JoinHandle<()>>, ()> {
    
    let mut thread_handler:Vec<thread::JoinHandle<()>> = vec![];
    for i in 0..num_threads {
        let alloc_threads = Arc::clone(&alloc_threads);
        let dealloc_threads = Arc::clone(&dealloc_threads);
        let dealloc_requests = Arc::clone(&dealloc_requests);
        let randomx_up_counter = Arc::clone(&randomx_up_counter);
        
        let sender = tx.clone();

        let reg_name:String = match reg_names {
            Some(ref n) => n[i as usize].to_string(),
            None => mocks::ThreadId::new(&*PEERID, &i).to_hex(),
        };

        // let reg_name = mocks::ThreadId::new(&*PEERID, &i).to_hex();
        let builder = thread::Builder::new().name(reg_name.clone());

        thread_handler.push(builder.spawn(move || {

            // register thread .. maybe
            let mut guard = alloc_threads.lock().unwrap();
            let v = &mut guard;
            v.push(reg_name);
            drop(guard);
            randomx_fast_instance(
                &key_block, 
                &*PEERID, 
                &sender, 
                &PUZZLE_DIFFICULTY.load(Relaxed),
                alloc_threads,
                dealloc_threads,
                dealloc_requests,
                randomx_up_counter,
            );

        }).unwrap());
        log::info!("randomx thread {} is up", i);
    }
    Ok(thread_handler)
}

pub fn randomx_verifier(signed_context: &Vec<u8>, nonce: &Vec<u8>, difficulty: &u32, puzzle_hash: &Vec<u8>) -> bool {

    let context = Arc::new(Context::new(signed_context, false));
    let hasher = Hasher::new(context); // new machine based on K
    let out = hasher.hash(&nonce); // we only need the first program which we init with the nonce
    let valid = out.leading_zeros() == *difficulty; // check if the fast hash meets the difficulty
    let out_bytes = out.as_ref().to_vec();

    let verified = out_bytes == *puzzle_hash;
    if valid && verified {
        return true;
    }
    false
}