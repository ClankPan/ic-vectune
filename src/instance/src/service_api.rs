use ic_cdk::{query, trap};
use ic_stable_structures::memory_manager::MemoryId;
use ssd_vectune::graph::UnorderedGraph;
use ssd_vectune::graph_store::GraphStore;
use vectune::PointInterface;

use crate::{consts::*, thread_locals::*, types::*};


use crate::point::Point;

/*


wip

距離はcos simに変更する

*/

#[query]
fn search(query_vector: Vec<f32>, top_k: u64, size_l: u64) -> Vec<(f32, u32)> {
    // ic_cdk::println!("{}\n{}", usize::MAX, u64::MAX);

    // ic_cdk::println!("stable_size u32: {}", ic_cdk::api::stable::stable_size() * WASM_PAGE_SIZE);

    let _start = ic_cdk::api::time();
    ic_cdk::println!("time: {}", ic_cdk::api::time());

    assert!(top_k <= size_l);

    METADATA.with(|metadata| {
        let metadata = metadata.borrow();
        let Metadata::Running(metadata) = &*metadata.get() else {
            trap("Metadata is not Running")
        };

        let storage_mem =
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(VAMANA_GRAPH_MEMORY_ID)));
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

        // let mut _rng = SmallRng::seed_from_u64(ic_cdk::api::time());

        // let ((k_ann, _visited), _) = vectune::search_with_optimal_stopping(&mut graph, &Point::from_f32_vec(query_vector), top_k as usize, &mut rng);
        let (k_ann, _visited) = vectune::search(
            &mut graph,
            &Point::from_f32_vec(query_vector),
            top_k as usize,
        );

        // let time = ic_cdk::api::time() - start;

        // ic_cdk::println!("visited len: {}, time: {}", visited.len(), ic_cdk::api::time());
        ic_cdk::println!("time: {}", ic_cdk::api::time());

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

        let storage_mem =
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(VAMANA_GRAPH_MEMORY_ID)));
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

        let (k_ann, visited) = vectune::search(
            &mut graph,
            &SIMDPoint::from_f32_vec(query_vector),
            top_k as usize,
        );

        ic_cdk::println!("visited len: {}", visited.len());

        k_ann
    })
}

#[query]
fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}
