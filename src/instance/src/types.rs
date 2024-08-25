use candid::{CandidType, Decode, Deserialize, Encode};
use ic_cdk::api::management_canister::main::CanisterStatusResponse;
use ic_cdk::trap;
use ic_stable_structures::memory_manager::VirtualMemory;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::Memory;
use ic_stable_structures::{DefaultMemoryImpl, Storable};
use num_traits::ToPrimitive;
pub use simd_point::Point as SIMDPoint;
use ssd_vectune::storage::StorageTrait;
use std::borrow::Cow;

use crate::simd_point;

use crate::thread_locals::*;

#[derive(CandidType, Deserialize, Clone)]
pub struct InitialMetadata {
    pub(crate) name: String,
    pub(crate) version: String,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct LoadingMetadata {
    // uploaded_chunks: BitVec<u8, Lsb0>,
    pub(crate) uploaded_graph_chunks: Vec<u8>, // serialized BitVec
    pub(crate) uploaded_datamap_chunks: Vec<u8>, // serialized BitVec
    pub(crate) uploaded_backlinks_chunks: Vec<u8>, // serialized BitVec
    pub(crate) chunk_byte_size: u64,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) db_key: String,

    pub(crate) medoid_node_index: u32,
    pub(crate) sector_byte_size: u64,
    pub(crate) num_vectors: u64,
    pub(crate) vector_dim: u64,
    pub(crate) edge_degrees: u64,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct RunningMetadata {
    pub(crate) name: String,
    pub(crate) version: String,

    pub(crate) medoid_node_index: u32,
    pub(crate) sector_byte_size: u64,
    pub(crate) num_vectors: u64,
    pub(crate) vector_dim: u64,
    pub(crate) edge_degrees: u64,
}

#[derive(CandidType, Deserialize, Clone)]
pub enum Metadata {
    None,
    Initial(InitialMetadata),
    Loading(LoadingMetadata),
    Running(RunningMetadata),
}

impl Metadata {
    pub fn update(new_metadata: Metadata) {
        METADATA.with(|m| {
            let _ = m.borrow_mut().set(new_metadata);
        });
    }
    pub fn get() -> Metadata {
        let metadata = METADATA.with(|m| m.borrow_mut().get().clone());
        metadata
    }

    pub fn get_name() -> String {
        let name = match Metadata::get() {
            Metadata::Initial(m) => m.name,
            Metadata::Loading(m) => m.name,
            Metadata::Running(m) => m.name,
            _ => trap(""),
        };
        name
    }

    pub fn get_version() -> String {
        let version = match Metadata::get() {
            Metadata::Initial(m) => m.version,
            Metadata::Loading(m) => m.version,
            Metadata::Running(m) => m.version,
            _ => trap(""),
        };
        version
    }

    pub fn get_db_key() -> Result<String, ()> {
        let db_key = match Metadata::get() {
            Metadata::Loading(m) => m.db_key,
            _ => return Err(()),
        };
        Ok(db_key)
    }

    pub fn change_version(version: String) {
        let metadata = match Metadata::get() {
            Metadata::Initial(mut m) => {
                m.version = version;
                Metadata::Initial(m)
            }
            Metadata::Loading(mut m) => {
                m.version = version;
                Metadata::Loading(m)
            }
            Metadata::Running(mut m) => {
                m.version = version;
                Metadata::Running(m)
            }
            _ => trap(""),
        };
        Metadata::update(metadata);
    }

    pub fn change_name(name: String) {
        let metadata = match Metadata::get() {
            Metadata::Initial(mut m) => {
                m.name = name;
                Metadata::Initial(m)
            }
            Metadata::Loading(mut m) => {
                m.name = name;
                Metadata::Loading(m)
            }
            Metadata::Running(mut m) => {
                m.name = name;
                Metadata::Running(m)
            }
            _ => trap(""),
        };
        Metadata::update(metadata);
    }

    pub fn change_db_key(db_key: String) -> Result<(), ()> {
        let metadata = match Metadata::get() {
            Metadata::Loading(mut m) => {
                m.db_key = db_key;
                Metadata::Loading(m)
            }
            _ => return Err(()),
        };
        Metadata::update(metadata);
        Ok(())
    }
}

impl Storable for Metadata {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 125_000_000, // max size, 1 billion bits
        is_fixed_size: false,
    };
}

pub struct Storage {
    pub(crate) storage_mem: VirtualMemory<DefaultMemoryImpl>,
}

impl StorageTrait for Storage {
    fn read(&self, offset: u64, dst: &mut [u8]) {
        // assert!(ic_cdk::api::stable::stable_size() * WASM_PAGE_SIZE <= offset as u64);
        // ic_cdk::println!("self.storage_mem.size() : {}", self.storage_mem.size() * WASM_PAGE_SIZE);
        // assert!(self.storage_mem.size() * WASM_PAGE_SIZE <= offset as u64);
        // ic_cdk::println!("read offset: {offset}, dst: {}", dst.len());
        self.storage_mem.read(offset as u64, dst);
    }

    fn write(&self, _offset: u64, _src: &[u8]) {
        todo!()
    }
}

#[derive(CandidType)]
pub struct ResponseSearchQuery {
    k_ann: Vec<(f32, u32)>,
    visited: Vec<(f32, u32)>,
    time: u64,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct IcStatus {
    pub(crate) controllers: Vec<String>,
    /// Compute allocation.
    pub(crate) compute_allocation: u128,
    /// Memory allocation.
    pub(crate) memory_allocation: u128,
    /// Freezing threshold.
    pub(crate) freezing_threshold: u128,
    /// A SHA256 hash of the module installed on the canister. This is null if the canister is empty.
    pub(crate) module_hash: Option<Vec<u8>>,
    /// The memory size taken by the canister.
    pub(crate) memory_size: u128,
    /// The cycle balance of the canister.
    pub(crate) cycles: u128,
    /// Amount of cycles burned per day.
    pub(crate) idle_cycles_burned_per_day: u128,
}

#[derive(CandidType)]
pub struct DbMetadata {
    pub(crate) name: String,
    pub(crate) owners: Vec<String>,
    pub(crate) cycle_amount: u64,
    pub(crate) stable_memory_size: u32,
    pub(crate) version: String,

    // For db uploading
    pub(crate) db_key: String,
    pub(crate) is_deserialized: bool,
    pub(crate) is_complete_hnsw_chunks: bool,
    pub(crate) is_complete_source_chunks: bool,
}

impl IcStatus {
    pub fn new(res: CanisterStatusResponse) -> Self {
        Self {
            controllers: res
                .settings
                .controllers
                .into_iter()
                .map(|p| p.to_string())
                .collect::<Vec<String>>(),
            compute_allocation: res.settings.compute_allocation.0.to_u128().unwrap(),
            memory_allocation: res.settings.memory_allocation.0.to_u128().unwrap(),
            freezing_threshold: res.settings.freezing_threshold.0.to_u128().unwrap(),
            module_hash: res.module_hash,
            memory_size: res.memory_size.0.to_u128().unwrap(),
            cycles: res.cycles.0.to_u128().unwrap(),
            idle_cycles_burned_per_day: res.idle_cycles_burned_per_day.0.to_u128().unwrap(),
        }
    }

    pub fn update(status: &IcStatus) {
        IC_STATUS.with(|s| {
            s.borrow_mut().insert(0, status.clone());
        });
    }

    pub fn get() -> IcStatus {
        let status = IC_STATUS.with(|s| s.borrow_mut().get(&0).clone().unwrap());
        status
    }
}

impl Storable for IcStatus {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 1024 * 1204,
        is_fixed_size: false,
    };
}

#[derive(CandidType, Deserialize, Clone)]
pub enum ChunkType {
    Graph,
    DataMap,
    BacklinksMap
}


type HeaderField = ( String, String );

#[derive(CandidType, candid::Deserialize, Debug)]
pub struct HttpRequest {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) headers: Vec<HeaderField>,
    pub(crate) body: Vec<u8>
}
#[derive(CandidType, candid::Deserialize)]
pub struct HttpResponse {
    pub(crate) status_code: u16,
    pub(crate) headers: Vec<HeaderField>,
    pub(crate) body: Vec<u8>
}

// #[derive(serde::Serialize, serde::Deserialize)]
// struct SearchQuery(Vec<f32>);

#[derive(CandidType)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SearchResponse {
  pub(crate) similarity: f32,
  pub(crate) data: String
}

#[derive(candid::CandidType, candid::Deserialize, Clone, Debug)]
pub struct Backlinks(pub Vec<u32>);

impl Storable for Backlinks {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    // const BOUND: Bound = Bound::Bounded {
    //     max_size: 125_000_000, // max size, 1 billion bits
    //     is_fixed_size: false,
    // };
    const BOUND: Bound = Bound::Unbounded;
}