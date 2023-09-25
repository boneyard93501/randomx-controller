use serde::{ Deserialize, Serialize };
use serde_json;
use std::cmp::PartialEq;
use std::fs::File;
use std::io::BufReader;

use crate::LOG_PATH;
use crate::RUNTME_CFG_PATH;
use crate::SETUP_CFG_PATH;
use crate::puzzle::PuzzleType;

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RXThreading {
    SINGLE,
    MULTI,
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct RandomxCfg {
    pub num_cores: u32,
    pub threads_per_core: u32,
    pub keypair: String,
    pub thread_model: RXThreading,
    pub puzzle: PuzzleType,
    pub difficulty: u32,
    pub key_blockchain_uri: String,
}

impl RandomxCfg {
    pub fn from_file() -> Result<Self, ()> {
        let file = File::open(SETUP_CFG_PATH).unwrap();
        let reader = BufReader::new(file);
        let cfg: RandomxCfg = serde_json::from_reader(reader).unwrap();
        if cfg.thread_model != RXThreading::SINGLE {
            log::error!("invalid thread model. only single is currently implemented.");
            panic!("{}", format!("invalid thread model config. see log {}.", LOG_PATH));
        }
        if cfg.puzzle != PuzzleType::ZEROS {
            log::error!("invalid puzzle model. only leading zeros is currently implemented.");
            panic!("{}", format!("invalid puzzle specified. see log {}", LOG_PATH));
        }
        if cfg.num_cores < 1 || cfg.threads_per_core < 1 {
            log::error!("invalid capacity allocation. check you core and thread counts.");
            panic!(
                "{}",
                format!(
                    "invalid capacity allocation provided. see {} and log path {}",
                    SETUP_CFG_PATH,
                    LOG_PATH
                )
            );
        }

        Ok(cfg)
    }
}

#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
pub struct RuntimeCfg {
    pub deallocated_threads: u32,
    operator_update: i64,
}

impl RuntimeCfg {
    pub fn from_file() -> Result<Self, ()> {
        let file = File::open(RUNTME_CFG_PATH).unwrap();
        let reader = BufReader::new(file);
        let cfg: RuntimeCfg = serde_json::from_reader(reader).unwrap();
        Ok(cfg)
    }
}
