pub mod ic_types;
pub mod simd_point;

use bitvec::prelude::*;
use candid::Principal;
use ic_cdk::{query, trap, update};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Storable};
use ic_stable_structures::Memory;
use candid::{CandidType, Decode, Deserialize, Encode};
use ssd_vectune::graph::UnorderedGraph;
use ssd_vectune::{graph_store::GraphStore, point::Point, storage::StorageTrait};
use vectune::PointInterface;
use std::borrow::Cow;
use std::cell::RefCell;
use bytesize::MIB;

use simd_point::Point as SIMDPoint;

/* Set custom random function */
use rand::rngs::{SmallRng, StdRng};
use rand::{thread_rng, Rng, RngCore, SeedableRng};
use getrandom::register_custom_getrandom;
// See here : https://forum.dfinity.org/t/issue-about-generate-random-string-panicked-at-could-not-initialize-thread-rng-getrandom-this-target-is-not-supported/15198/8?u=kinicdevcontributor
fn custom_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
  RNG.with(|rng| rng.borrow_mut().fill_bytes(buf));
  Ok(())
}
register_custom_getrandom!(custom_getrandom);


const WASM_PAGE_SIZE: u64 = 65536;
const MISSING_CHUNKS_RESPONCE_SIZE: usize = 2 * MIB as usize;
// const MISSING_CHUNKS_RESPONCE_SIZE: usize = 10 as usize;

thread_local! {
    // The memory manager is used for simulating multiple memories. Given a `MemoryId` it can
    // return a memory that can be used by stable structures.
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
      RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
}

thread_local! {
    static METADATA:    RefCell<StableCell::<Metadata, VirtualMemory<DefaultMemoryImpl>>>= RefCell::new(
        StableCell::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1))),
            Metadata::None
        ).unwrap()
    );
    static RNG:         RefCell<StdRng> = RefCell::new(StdRng::from_seed(thread_rng().gen()));
}

#[derive(CandidType, Deserialize, Clone)]
struct LoadingMetadata {
    // uploaded_chunks: BitVec<u8, Lsb0>,
    uploaded_chunks: Vec<u8>, // serialized BitVec
    chunk_byte_size: u64,

    medoid_node_index: u32,
    sector_byte_size: u64,
    num_vectors: u64,
    vector_dim: u64,
    edge_degrees: u64,
}

#[derive(CandidType, Deserialize, Clone)]
struct RunningMetadata {
    medoid_node_index: u32,
    sector_byte_size: u64,
    num_vectors: u64,
    vector_dim: u64,
    edge_degrees: u64,
}

#[derive(CandidType, Deserialize, Clone)]
enum Metadata {
    None,
    Loading(LoadingMetadata),
    Running(RunningMetadata),
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

struct Storage {
    storage_mem: VirtualMemory<DefaultMemoryImpl>,
    sector_byte_size: usize,
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

    fn sector_byte_size(&self) -> usize {
        self.sector_byte_size
    }
}

#[query]
fn status_code() -> u8 {
    METADATA.with(|metadata| {
        let metadata = metadata.borrow();

        match *metadata.get() {
            Metadata::None => {
                0
            },
            Metadata::Loading(_) => {
                1
            },
            Metadata::Running(_) => {
                2
            }
        }
    })
}

async fn get_controllers() -> Vec<Principal> {
    let status: ic_types::CanisterStatusResponse =
        ic_types::canister_status(ic_types::CanisterIdRecord {
            canister_id: ic_cdk::id(),
        })
        .await
        .unwrap()
        .0;

        status.settings.controllers
}

async fn assert_owner() {
    let controllers = get_controllers().await;
    if !is_owner(&controllers) {
        trap("You are not controller")
    }
}

#[update]
async fn initialize(
    num_chunks: u64,
    chunk_byte_size: u64,
    medoid_node_index: u32,
    sector_byte_size: u64,
    num_vectors: u64,
    vector_dim: u64,
    edge_degrees: u64,
) {
    // let status: ic_types::CanisterStatusResponse =
    //     ic_types::canister_status(ic_types::CanisterIdRecord {
    //         canister_id: ic_cdk::id(),
    //     })
    //     .await
    //     .unwrap()
    //     .0;
    // let controllers = Rc::new(status.settings.controllers);

    assert_owner().await;

    METADATA.with(|metadata| {
        let mut metadata = metadata.borrow_mut();
        let Metadata::None = *metadata.get() else {
            trap("Metadata is not None")
        };

        let _ = metadata.set(Metadata::Loading(LoadingMetadata {
            uploaded_chunks: bincode::serialize(&bitvec![u8, Lsb0; 0; num_chunks as usize]).unwrap(),
            chunk_byte_size,

            medoid_node_index,
            sector_byte_size,
            num_vectors,
            vector_dim,
            edge_degrees,
        }));
    });

    let storage_mem = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0)));
    let num_pages = (num_chunks * chunk_byte_size + WASM_PAGE_SIZE - 1) / WASM_PAGE_SIZE;

    // wip 現在のページ数

    storage_mem.grow(num_pages);
}

#[update]
async fn upload_chunk(chunk: Vec<u8>, chunk_index: u64) {
    assert_owner().await;

    METADATA.with(|metadata| {
        let mut metadata = metadata.borrow_mut();
        let Metadata::Loading(mut loading_metadata) = metadata.get().clone() else {
            trap("Metadata is not Loading")
        };
        let mut uploaded_chunks: BitVec<u8, Lsb0> = bincode::deserialize(&loading_metadata.uploaded_chunks).unwrap();

        assert!(chunk.len() <= loading_metadata.chunk_byte_size as usize);

        let storage_mem = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0)));
        let offset = loading_metadata.chunk_byte_size * chunk_index;
        let src = &chunk[..];

        storage_mem.write(offset, src);
        uploaded_chunks.set(chunk_index as usize, true);

        loading_metadata.uploaded_chunks = bincode::serialize(&uploaded_chunks).unwrap();

        let _ = metadata.set(Metadata::Loading(loading_metadata));
    })
}

#[query]
fn missing_chunks(section: u64) -> Option<Vec<u8>> {

    METADATA.with(|metadata| {
        let metadata = metadata.borrow();
        let Metadata::Loading(loading_metadata) = &*metadata.get() else {
            trap("Metadata is not Loading")
        };

        let start = MISSING_CHUNKS_RESPONCE_SIZE * section as usize;

        if start >= loading_metadata.uploaded_chunks.len() {
            return None
        }

        let end = std::cmp::min(start + MISSING_CHUNKS_RESPONCE_SIZE, loading_metadata.uploaded_chunks.len());

        Some(loading_metadata.uploaded_chunks[start..end].to_vec())
    })
}

#[update]
async fn start() {
    assert_owner().await;

    METADATA.with(|metadata| {
        let mut metadata = metadata.borrow_mut();
        let Metadata::Loading(loading_metadata) = metadata.get().clone() else {
            trap("Metadata is not Loading")
        };

        let uploaded_chunks: BitVec<u8, Lsb0> = bincode::deserialize(&loading_metadata.uploaded_chunks).unwrap();

        if uploaded_chunks.iter().all(|bit| *bit) {
            let _ = metadata.set(Metadata::Running(RunningMetadata {
                medoid_node_index: loading_metadata.medoid_node_index,
                sector_byte_size: loading_metadata.sector_byte_size,
                num_vectors: loading_metadata.num_vectors,
                vector_dim: loading_metadata.vector_dim,
                edge_degrees: loading_metadata.edge_degrees,
            }));
        } else {
            trap("uploading chunk is not done")
        }
    })
}

#[update]
async fn reset() {
    assert_owner().await;
    todo!();

    // METADATA.with(|metadata| {
    //     let mut metadata = metadata.borrow_mut();
    //     let Metadata::Running(running_metadata) = &mut *metadata.get() else {
    //         trap("Metadata is not Running")
    //     };

    // })
}

#[derive(CandidType)]
pub struct ResponseSearchQuery {
    k_ann: Vec<(f32, u32)>,
    visited: Vec<(f32, u32)>,
    time: u64,
}

#[query]
fn search(query_vector: Vec<f32>, top_k: u64, size_l: u64) -> Vec<(f32, u32)> {

    // ic_cdk::println!("{}\n{}", usize::MAX, u64::MAX);

    // ic_cdk::println!("stable_size u32: {}", ic_cdk::api::stable::stable_size() * WASM_PAGE_SIZE);

    let start = ic_cdk::api::time();
    ic_cdk::println!("time: {}",ic_cdk::api::time());

    assert!(top_k <= size_l);

    METADATA.with(|metadata| {
        let metadata = metadata.borrow();
        let Metadata::Running(metadata) = &*metadata.get() else {
            trap("Metadata is not Running")
        };

        let storage_mem = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0)));
        let storage = Storage {
            storage_mem,
            sector_byte_size: metadata.sector_byte_size as usize,
        };

        let unordered_graph_on_storage = GraphStore::new(
            metadata.num_vectors as usize,
            metadata.vector_dim as usize,
            metadata.edge_degrees as usize,
            storage,
        );

        let mut graph = UnorderedGraph::new(unordered_graph_on_storage, metadata.medoid_node_index);

        graph.set_size_l(size_l as usize);

        let mut rng = SmallRng::seed_from_u64(ic_cdk::api::time());

        // let ((k_ann, _visited), _) = vectune::search_with_optimal_stopping(&mut graph, &Point::from_f32_vec(query_vector), top_k as usize, &mut rng);
        let (k_ann, _visited)= vectune::search(&mut graph, &Point::from_f32_vec(query_vector), top_k as usize);

        // let time = ic_cdk::api::time() - start;

        // ic_cdk::println!("visited len: {}, time: {}", visited.len(), ic_cdk::api::time());
        ic_cdk::println!("time: {}",ic_cdk::api::time());

        k_ann
    })
}

#[query]
fn search_with_simd(query_vector: Vec<f32>, top_k: u64, size_l: u64) -> Vec<(f32, u32)> {

    // ic_cdk::println!("{}\n{}", usize::MAX, u64::MAX);

    // ic_cdk::println!("stable_size u32: {}", ic_cdk::api::stable::stable_size() * WASM_PAGE_SIZE);

    assert!(top_k <= size_l);

    METADATA.with(|metadata| {
        let metadata = metadata.borrow();
        let Metadata::Running(metadata) = &*metadata.get() else {
            trap("Metadata is not Running")
        };

        let storage_mem = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0)));
        let storage = Storage {
            storage_mem,
            sector_byte_size: metadata.sector_byte_size as usize,
        };

        let unordered_graph_on_storage = GraphStore::new(
            metadata.num_vectors as usize,
            metadata.vector_dim as usize,
            metadata.edge_degrees as usize,
            storage,
        );

        let mut graph = UnorderedGraph::new(unordered_graph_on_storage, metadata.medoid_node_index);

        graph.set_size_l(size_l as usize);

        let (k_ann, visited) = vectune::search(&mut graph, &SIMDPoint::from_f32_vec(query_vector), top_k as usize);

        ic_cdk::println!("visited len: {}", visited.len());

        k_ann
    })
}

fn is_owner(controllers: &Vec<Principal>) -> bool {
    let caller = ic_cdk::caller();
    controllers.contains(&caller)
}

#[query]
fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}

/* !!Should be end of this file!! */
// Enable Candid export
// cargo build --release --target wasm32-unknown-unknown --package instance
// candid-extractor target/wasm32-unknown-unknown/release/instance.wasm > src/instance/instance.did
ic_cdk::export_candid!();