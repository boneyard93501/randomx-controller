use serde::{Deserialize, Serialize};
use std::fs;
use serde_json;
use hex;
use crate::PUZZLE_SOLUTION_DIR as DIR;

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PuzzleType {
    ZEROS,
    // COMP,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PuzzleSolution {
    pub peer_id: Vec<u8>,
    pub key_block: u64,
    pub signed_context: Vec<u8>,
    pub thread_name: Vec<u8>,
    pub nonce: Vec<u8>,
    pub signed_nonce: Vec<u8>,
    pub hash: Vec<u8>,
    pub difficulty: u32,
}

impl PuzzleSolution {
    pub fn new(peer_id: Vec<u8>, key_block: u64, signed_context:Vec<u8>, thread_name:Vec<u8>, nonce:Vec<u8>, signed_nonce:Vec<u8>, hash:Vec<u8>, difficulty: u32) -> Self {
        PuzzleSolution {
            peer_id,
            key_block,
            signed_context,
            thread_name,
            nonce,
            signed_nonce,
            hash,
            // puzzle_type            // need to add if that's going to be dynamic
            difficulty      // need to add if that's going to be dynamic
        }
    }

    pub fn to_file(&self, out_dir: Option<&str>) -> Result<(), String> {
        let out = match out_dir {
            Some(d) => d,
            None => DIR,
        };

        let fname = hex::encode(self.hash.clone());

        let path:String = match out.ends_with("/") {
            true => format!("{}{}.json", out, fname ),
            false => format!("{}/{}.json", out, fname),
        };

        let writer = match fs::File::create(path) {
            Ok(w) => w,
            Err(e) => { return Err(format!("{}", e)); }
        };

        // let writer = File::open(path).unwrap();
        match serde_json::to_writer(writer, self) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("{}", e))
        }
    }

}