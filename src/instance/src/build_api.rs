use crate::{auth::*, consts::*, service_api::*, thread_locals::*, types::*};
use bitvec::prelude::*;
use candid::Principal;
use ic_cdk::api::management_canister::main::{CanisterIdRecord, CanisterStatusResponse};
use ic_cdk::{query, trap, update};
use ic_stable_structures::memory_manager::MemoryId;
use ic_stable_structures::Memory;

#[query]
fn status_code() -> u8 {
    METADATA.with(|metadata| {
        let metadata = metadata.borrow();

        match *metadata.get() {
            Metadata::Initial(_) => 0,
            Metadata::Loading(_) => 1,
            Metadata::Running(_) => 2,
            _ => trap(""),
        }
    })
}

#[update(guard = "is_owner")]
async fn start_loading(
    num_graph_chunks: u64,
    num_datamap_chunks: u64,
    num_backlinks_chunks: u64,
    chunk_byte_size: u64,
    _medoid_node_index: u32,
    _sector_byte_size: u64,
    _num_vectors: u64,
    _vector_dim: u64,
    _edge_degrees: u64,
    db_key: String,
) {
    METADATA.with(|metadata| {
        let mut metadata = metadata.borrow_mut();

        let (name, version) = match metadata.get() {
            Metadata::Initial(initial_metadata) => (
                initial_metadata.name.clone(),
                initial_metadata.version.clone(),
            ),
            _ => trap("Metadata is not initial"),
        };

        let _ = metadata.set(Metadata::Loading(LoadingMetadata {
            uploaded_graph_chunks: bincode::serialize(
                &bitvec![u8, Lsb0; 0; num_graph_chunks as usize],
            )
            .unwrap(),
            uploaded_datamap_chunks: bincode::serialize(
                &bitvec![u8, Lsb0; 0; num_datamap_chunks as usize],
            )
            .unwrap(),
            uploaded_backlinks_chunks: bincode::serialize(
                &bitvec![u8, Lsb0; 0; num_backlinks_chunks as usize],
            )
            .unwrap(),
            chunk_byte_size,
            name,
            version,
            db_key,
            // medoid_node_index,
            // sector_byte_size,
            // num_vectors,
            // vector_dim,
            // edge_degrees,
        }));
    });

    // 現在のページ数を計算して、正しいページ数をgrowしないといけない。

    // graph
    grow_page_size(chunk_byte_size, num_graph_chunks, VAMANA_GRAPH_MEMORY_ID);

    // data-map
    grow_page_size(chunk_byte_size, num_datamap_chunks, DATA_MAP_MEMORY_ID);

    // backlinks
    grow_page_size(chunk_byte_size, num_backlinks_chunks, BACKLINKS_MEMORY_ID);
}

fn grow_page_size(chunk_byte_size: u64, num_chunks: u64, memory_id: u8) {
    let storage_mem = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(memory_id)));
    let num_pages = (num_chunks * chunk_byte_size + WASM_PAGE_SIZE - 1) / WASM_PAGE_SIZE;
    let current_page_size = storage_mem.size();
    if num_pages > current_page_size {
        storage_mem.grow(num_pages - current_page_size);
    }
}

#[update(guard = "is_owner")]
async fn upload_chunk(chunk: Vec<u8>, chunk_index: u64, chunk_type: ChunkType) {
    METADATA.with(|metadata| {
        let mut metadata = metadata.borrow_mut();
        let Metadata::Loading(mut loading_metadata) = metadata.get().clone() else {
            trap("Metadata is not Loading")
        };
        let (uploaded_chunks_serialized, memory_id): (&mut Vec<u8>, u8) = match chunk_type {
            ChunkType::Graph => (
                &mut loading_metadata.uploaded_graph_chunks,
                VAMANA_GRAPH_MEMORY_ID,
            ),
            ChunkType::DataMap => (
                &mut loading_metadata.uploaded_datamap_chunks,
                DATA_MAP_MEMORY_ID,
            ),
            ChunkType::BacklinksMap => (
                &mut loading_metadata.uploaded_backlinks_chunks,
                BACKLINKS_MEMORY_ID,
            ),
        };
        let mut uploaded_chunks: BitVec<u8, Lsb0> =
            bincode::deserialize(&uploaded_chunks_serialized).unwrap();

        assert!(chunk.len() <= loading_metadata.chunk_byte_size as usize);

        let storage_mem = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(memory_id)));
        let offset = loading_metadata.chunk_byte_size * chunk_index;
        let src = &chunk[..];

        storage_mem.write(offset, src);
        uploaded_chunks.set(chunk_index as usize, true);
        *uploaded_chunks_serialized = bincode::serialize(&uploaded_chunks).unwrap();

        let _ = metadata.set(Metadata::Loading(loading_metadata));
    })
}

#[query]
fn missing_chunks(section: u64, chunk_type: ChunkType) -> Option<Vec<u8>> {
    METADATA.with(|metadata| {
        let metadata = metadata.borrow();
        let Metadata::Loading(loading_metadata) = &*metadata.get() else {
            trap("Metadata is not Loading")
        };
        let uploaded_chunks: &Vec<u8> = match chunk_type {
            ChunkType::Graph => &loading_metadata.uploaded_graph_chunks,
            ChunkType::DataMap => &loading_metadata.uploaded_datamap_chunks,
            ChunkType::BacklinksMap => &loading_metadata.uploaded_backlinks_chunks,
        };

        let start = MISSING_CHUNKS_RESPONCE_SIZE * section as usize;

        if start >= uploaded_chunks.len() {
            return None;
        }

        let end = std::cmp::min(start + MISSING_CHUNKS_RESPONCE_SIZE, uploaded_chunks.len());

        Some(uploaded_chunks[start..end].to_vec())
    })
}

#[update(guard = "is_owner")]
async fn start_running() {
    METADATA.with(|metadata| {
        let mut metadata = metadata.borrow_mut();
        let Metadata::Loading(loading_metadata) = metadata.get().clone() else {
            trap("Metadata is not Loading")
        };

        let uploaded_graph_chunks: BitVec<u8, Lsb0> =
            bincode::deserialize(&loading_metadata.uploaded_graph_chunks).unwrap();

        let uploaded_datamap_chunks: BitVec<u8, Lsb0> =
            bincode::deserialize(&loading_metadata.uploaded_datamap_chunks).unwrap();

        let uploaded_backlinks_chunks: BitVec<u8, Lsb0> =
            bincode::deserialize(&loading_metadata.uploaded_backlinks_chunks).unwrap();

        // let is_done = uploaded_graph_chunks.iter().all(|bit| *bit) && uploaded_datamap_chunks.iter().all(|bit| *bit) && uploaded_backlinks_chunks.iter().all(|bit| *bit);
        let is_done = uploaded_graph_chunks
            .iter()
            .chain(uploaded_datamap_chunks.iter())
            .chain(uploaded_backlinks_chunks.iter())
            .all(|bit| *bit);

        let graph = load_graph();
        let num_vectors = graph.graph_store.num_vectors();

        if is_done {
            let _ = metadata.set(Metadata::Running(RunningMetadata {
                name: loading_metadata.name,
                version: loading_metadata.version,
                // medoid_node_index: loading_metadata.medoid_node_index,
                // sector_byte_size: loading_metadata.sector_byte_size,
                // num_vectors: loading_metadata.num_vectors,
                // vector_dim: loading_metadata.vector_dim,
                // edge_degrees: loading_metadata.edge_degrees,
                current_max_unsed_index: num_vectors.try_into().expect("cannot convert u64 to u32"),
            }));
        } else {
            trap("uploading chunk is not done")
        }
    })
}

#[update(guard = "is_owner")]
async fn reset() {
    METADATA.with(|metadata| {
        let mut metadata = metadata.borrow_mut();

        let new_metaadta = match metadata.get() {
            Metadata::None => panic!("unexpected path"),
            Metadata::Initial(_) => return,
            Metadata::Loading(loading_metadata) => Metadata::Initial(InitialMetadata {
                name: loading_metadata.name.clone(),
                version: loading_metadata.version.clone(),
            }),
            Metadata::Running(running_metadata) => Metadata::Initial(InitialMetadata {
                name: running_metadata.name.clone(),
                version: running_metadata.version.clone(),
            }),
        };

        let _ = metadata.set(new_metaadta);
    })
}

#[query(guard = "is_owner")]
fn get_prev_status() -> StatusForFrontend {
    let status = IcStatus::get();
    let name = Metadata::get_name();
    let version = Metadata::get_version();
    let db_key = match Metadata::get_db_key() {
        Ok(key) => key,
        Err(_) => String::new(),
    };

    StatusForFrontend {
        controllers: status.controllers,
        compute_allocation: status.compute_allocation,
        memory_allocation: status.memory_allocation,
        freezing_threshold: status.freezing_threshold,
        module_hash: status.module_hash,
        memory_size: status.memory_size,
        cycles: status.cycles,
        idle_cycles_burned_per_day: status.idle_cycles_burned_per_day,

        // For DB
        db_key,
        hnsw_chunk_len: 0,
        source_chunk_len: 0,
        name,
        version,
    }
}

#[query]
fn get_name() -> String {
    Metadata::get_name()
}

#[update(guard = "is_owner")]
fn change_name(new_name: String) {
    Metadata::change_name(new_name);
}

#[update(guard = "is_owner")]
fn add_new_owner(new_owner_pid: Principal) {
    add_owner(new_owner_pid, 1);
}

// #[update(guard = "is_owner")]
// fn set_db_keys(db_key: String, hnsw_chunk_len: u32, source_chunk_len: u32) {
//     Metadata::change_db_key(db_key);
//     Metadata::change_chunk_lens(hnsw_chunk_len, source_chunk_len);
// }

#[update]
fn commit() {
    if !(ic_cdk::caller() == ic_cdk::id()) {
        ic_cdk::trap("not self calling");
    }
    ic_cdk::println!("commit")
}

#[query(guard = "is_owner")]
fn get_metadata() -> DbMetadata {
    let name = Metadata::get_name();
    let version = Metadata::get_version();

    // wip https://doc.rust-lang.org/stable/core/arch/wasm/fn.memory_size.html

    let owners: Vec<String> =
        OWNERS.with(|owners| owners.borrow().iter().map(|(index, _)| index).collect());
    let cycle_amount = ic_cdk::api::canister_balance();
    let stable_memory_size: u32 = ic_cdk::api::stable::stable_size().try_into().unwrap();

    let is_deserialized: bool = false;
    let is_complete_hnsw_chunks = false;
    let is_complete_source_chunks = false;

    let db_key = match Metadata::get_db_key() {
        Ok(key) => key,
        Err(_) => String::new(),
    };

    DbMetadata {
        name,
        owners,
        cycle_amount,
        stable_memory_size,
        version,
        db_key,
        is_deserialized,
        is_complete_hnsw_chunks,
        is_complete_source_chunks,
    }
}

#[derive(candid::CandidType, candid::Deserialize, Clone)]
pub struct StatusForFrontend {
    controllers: Vec<String>,
    /// Compute allocation.
    compute_allocation: u128,
    /// Memory allocation.
    memory_allocation: u128,
    /// Freezing threshold.
    freezing_threshold: u128,
    /// A SHA256 hash of the module installed on the canister. This is null if the canister is empty.
    module_hash: Option<Vec<u8>>,
    /// The memory size taken by the canister.
    memory_size: u128,
    /// The cycle balance of the canister.
    cycles: u128,
    /// Amount of cycles burned per day.
    idle_cycles_burned_per_day: u128,

    // For DB
    db_key: String,
    hnsw_chunk_len: u32,
    source_chunk_len: u32,
    name: String,
    version: String,
}

#[update(guard = "is_owner")]
async fn get_current_status() -> StatusForFrontend {
    let status_row: CanisterStatusResponse =
        ic_cdk::api::management_canister::main::canister_status(CanisterIdRecord {
            canister_id: ic_cdk::id(),
        })
        .await
        .unwrap()
        .0;

    let status = IcStatus::new(status_row);
    IcStatus::update(&status);

    let name = Metadata::get_name();
    let version = Metadata::get_version();
    let db_key = match Metadata::get_db_key() {
        Ok(key) => key,
        Err(_) => String::new(),
    };

    StatusForFrontend {
        controllers: status.controllers,
        compute_allocation: status.compute_allocation,
        memory_allocation: status.memory_allocation,
        freezing_threshold: status.freezing_threshold,
        module_hash: status.module_hash,
        memory_size: status.memory_size,
        cycles: status.cycles,
        idle_cycles_burned_per_day: status.idle_cycles_burned_per_day,

        // For DB
        db_key,
        hnsw_chunk_len: 0,
        source_chunk_len: 0,
        name,
        version,
    }
}
