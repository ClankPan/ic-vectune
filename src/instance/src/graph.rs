use ic_cdk::trap;
use vectune::{GraphInterface, PointInterface};

use ssd_vectune::graph_store::GraphStore;
use ssd_vectune::storage::StorageTrait;

use crate::{consts::*, thread_locals::*, types::*, auth::*};

#[derive(Clone)]
pub struct Graph<S: StorageTrait> {
    size_l: usize,
    size_r: usize,
    size_a: f32,

    graph_store: GraphStore<S>,
    start_node_index: u32,
}

impl<S: StorageTrait> Graph<S> {
    pub fn new(graph_store: GraphStore<S>) -> Self {
        Self {
            size_l: 125,
            size_r: graph_store.max_edge_degrees(),
            size_a: 2.0,
            start_node_index: graph_store.start_id() as u32,
            graph_store,
        }
    }

    pub fn set_start_node_index(&mut self, index: u32) {
        self.start_node_index = index;
    }

    pub fn set_size_l(&mut self, size_l: usize) {
        self.size_l = size_l;
    }
}

impl<S: StorageTrait, P: PointInterface> GraphInterface<P> for Graph<S> {
    fn alloc(&mut self, _point: P) -> u32 {
        let new_index = METADATA.with(|metadata| {
            let mut metadata = metadata.borrow_mut();
            let Metadata::Running(mut running_metadata) = metadata.get().clone() else {
                trap("Metadata is not Running")
            };
            let current_max_index = running_metadata.num_vectors;
            running_metadata.num_vectors += 1;

            metadata.set(Metadata::Running(running_metadata)).expect("cannot set new metadata");
            current_max_index
        });

        new_index.try_into().expect("cannot convert u64 to u32")
    }

    fn free(&mut self, _id: &u32) {
        todo!()
        // todo: backlink
    }

    fn cemetery(&self) -> Vec<u32> {
        vec![]
    }

    fn clear_cemetery(&mut self) {
        todo!()
    }

    fn backlink(&self, id: &u32) -> Vec<u32> {
        let backlinks = BACKLINKS_MAP.with(|map| {
            map.borrow().get(id).expect("id does not exsit in backlinks map").0
        });
        backlinks
    }

    fn get(&mut self, node_index: &u32) -> (P, Vec<u32>) {
        let store_index = node_index;
        let (vector, edges) = self.graph_store.read_node(&store_index).unwrap();

        (P::from_f32_vec(vector), edges)
    }

    fn size_l(&self) -> usize {
        self.size_l
    }

    fn size_r(&self) -> usize {
        self.size_r
    }

    fn size_a(&self) -> f32 {
        self.size_a
    }

    fn start_id(&self) -> u32 {
        self.start_node_index
    }

    fn overwirte_out_edges(&mut self, _id: &u32, _edges: Vec<u32>) {
        todo!()
    }
}
