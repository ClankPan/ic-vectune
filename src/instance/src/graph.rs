use std::collections::HashSet;

use ic_cdk::trap;
use ic_stable_structures::{memory_manager::MemoryId, Vec as SVec};
use vectune::{GraphInterface, PointInterface};

use ssd_vectune::graph_store::GraphStore;
use ssd_vectune::storage::StorageTrait;

use crate::{thread_locals::*, types::*, CEMETERY_MEMORY_ID};

#[derive(Clone)]
pub struct Graph<S: StorageTrait> {
    size_l: usize,
    size_r: usize,
    size_a: f32,

    pub graph_store: GraphStore<S>,
    start_node_index: u32, // note: これは、headerに格納されているので、upgrade-stable
}

impl<S: StorageTrait> Graph<S> {
    pub fn suspect(&mut self, id: u32) {
        CEMETERY.with(|list| list.borrow_mut().push(&id).expect("faile to push item"));
    }
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
    fn alloc(&mut self, point: P) -> u32 {
        let new_index = METADATA.with(|metadata| {
            let mut metadata = metadata.borrow_mut();
            let Metadata::Running(mut running_metadata) = metadata.get().clone() else {
                trap("Metadata is not Running")
            };
            let new_index = running_metadata.current_max_unsed_index;
            running_metadata.current_max_unsed_index += 1;

            metadata
                .set(Metadata::Running(running_metadata))
                .expect("cannot set new metadata");
            new_index
        });

        self.graph_store
            .write_node(&new_index, &point.to_f32_vec(), &vec![])
            .expect("fail to write node");

        // let node = self.graph_store.read_node(&new_index).unwrap();
        // ic_cdk::println!("in alloc: {:?}", node);

        // wip todo storage上のheaderに書き込む
        // self.graph_store.set_num_vectors();

        BACKLINKS_MAP.with(|map| {
            let mut map = map.borrow_mut();
            map.insert(new_index, Backlinks(HashSet::new()));
        });

        // grow is called in storage.write()

        new_index.try_into().expect("cannot convert u64 to u32")
    }

    fn free(&mut self, id: &u32) {
        FREE_ID_LIST.with(|list| {
            let list = list.borrow_mut();
            list.push(id).expect("fail, push to free id list");
        });

        let (_, edges): (P, Vec<u32>) = self.get(id);
        let edge_set: HashSet<u32> = edges.into_iter().collect();

        BACKLINKS_MAP.with(|map| {
            let mut map = map.borrow_mut();

            for deleted_id in edge_set {
                let mut backlinks_set: HashSet<u32> =
                    map.get(&deleted_id).unwrap().0.into_iter().collect();
                backlinks_set.remove(id);
                map.insert(deleted_id, Backlinks(backlinks_set));
            }

            map.insert(*id, Backlinks(vec![].into_iter().collect()))
        });
    }

    fn cemetery(&self) -> Vec<u32> {
        CEMETERY.with(|list| {
            let list = list.borrow();
            let cemetery = list.iter().collect();
            cemetery
        })
    }

    fn clear_cemetery(&mut self) {
        SVec::<u32, VMemory>::new(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(CEMETERY_MEMORY_ID))),
        )
        .unwrap();
    }

    fn backlink(&self, id: &u32) -> Vec<u32> {
        let backlinks = BACKLINKS_MAP.with(|map| {
            map.borrow()
                .get(id)
                .expect("id does not exsit in backlinks map")
                .0
        });
        backlinks.into_iter().collect()
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

    fn overwirte_out_edges(&mut self, id: &u32, new_edges: Vec<u32>) {
        let (point, prev_edges): (P, Vec<u32>) = self.get(id);

        // ic_cdk::println!("id: {}\nprev_edges: {:?}\nnew_edges: {:?}", id, prev_edges, new_edges);

        self.graph_store
            .write_node(id, &point.to_f32_vec(), &new_edges)
            .expect("fail to write node");

        let prev_edge_set: HashSet<u32> = prev_edges.into_iter().collect();
        let new_edge_set: HashSet<u32> = new_edges.into_iter().collect();

        let deleted: HashSet<_> = prev_edge_set.difference(&new_edge_set).cloned().collect();
        let inserted: HashSet<_> = new_edge_set.difference(&prev_edge_set).cloned().collect();

        // ic_cdk::println!("inserted{:?}", inserted.clone().into_iter().collect::<Vec<u32>>());

        BACKLINKS_MAP.with(|map| {
            let mut map = map.borrow_mut();

            for deleted_id in deleted {
                let mut backlinks_set: HashSet<u32> =
                    map.get(&deleted_id).unwrap().0.into_iter().collect();
                backlinks_set.remove(id);
                map.insert(deleted_id, Backlinks(backlinks_set));
            }

            for inserted_id in inserted {
                let mut backlinks_set: HashSet<u32> =
                    map.get(&inserted_id).unwrap().0.into_iter().collect();
                backlinks_set.insert(*id);
                map.insert(inserted_id, Backlinks(backlinks_set));
            }
        });
    }
}
