use std::{cell::RefCell, sync::{Arc, RwLock}};

use wasm_bindgen::prelude::*;
use ic_stable_structures::{BTreeMap as StableBTreeMap, Memory};
use ssd_vectune::{graph_store::GraphStore, storage::StorageTrait, original_vector_reader::OriginalVectorReaderTrait};
use anyhow::Result;

const WASM_PAGE_SIZE: u64 = 65536;

/*

jsonを読み込んで、embeddingがないものだけをembeddして、call_backでそれぞれを渡す。

embedding -> vamana -> rbtree

*/

struct ICMemory {
//   mem: Vec<u8>,
    mem: RefCell<Vec<u8>>,
}

impl Memory for ICMemory {
    fn size(&self) -> u64 {
        let mem_len = self.mem.borrow().len() as u64;
        (mem_len * WASM_PAGE_SIZE - 1) / WASM_PAGE_SIZE
    }

    fn grow(&self, pages: u64) -> i64 {
        let current_page_size = self.size();
        if (current_page_size + pages) * WASM_PAGE_SIZE > (usize::MAX) as u64 {
            return -1;
        }

        let prev_page_size = current_page_size;
        self.mem.borrow_mut().extend(vec![0; (pages * WASM_PAGE_SIZE) as usize]);
        prev_page_size as i64
    }

    fn read(&self, offset: u64, dst: &mut [u8]) {
        let slice = &self.mem.borrow()[offset as usize..offset as usize + dst.len()];
        dst.copy_from_slice(slice);
    }

    fn write(&self, offset: u64, src: &[u8]) {
        let slice = &mut self.mem.borrow_mut()[offset as usize..offset as usize + src.len()];
        slice.copy_from_slice(src);
    }
}

pub struct ICRBTree {
    ic_rbtree: StableBTreeMap<u32, Vec<u8>, ICMemory>
}
impl ICRBTree {
    pub fn new() -> Self {
        let ic_rbtree = StableBTreeMap::new(ICMemory {mem: RefCell::new(vec![])});
        Self {
            ic_rbtree,
        }
    }

    pub fn insert(&mut self, key: u32, value: Vec<u8>) -> Option<Vec<u8>> {
        self.ic_rbtree.insert(key, value)
    }

    // pub fn build_from_string(&mut self, items: Vec<String>) {
    //     let items: Vec<Vec<u8>> = items.into_iter().map(|item| item.into_bytes()).collect();
    //     self.build_from_vec_u8(items)
    // }

    pub fn build_from_vec_u8(items: Vec<Vec<u8>>) -> Self {
        let mut ic_rbtree = StableBTreeMap::new(ICMemory {mem: RefCell::new(vec![])});
        for (index, item) in items.into_iter().enumerate() {
            let _ = ic_rbtree.insert(index.try_into().unwrap(), item);
        }

        Self {
            ic_rbtree,
        }
    }

    pub fn into_memory(self) -> Vec<u8> {
        self.ic_rbtree.into_memory().mem.into_inner()
    }
}

struct Storage {
    // mem: RefCell<Vec<u8>>
    mem: Arc<RwLock<Vec<u8>>>,
}

impl Storage {
    pub fn new(file_byte_size: u64) -> Self {
        Self {
            // mem: RefCell::new(vec![0; file_byte_size.try_into().unwrap()]),
            mem: Arc::new(RwLock::new(vec![0; file_byte_size.try_into().unwrap()])),
        }
    }

    pub fn into_memory(self) -> Vec<u8> {
        // self.mem.into_inner().unwrap()
        Arc::try_unwrap(self.mem)
            .expect("Arc still has other references")
            .into_inner()
            .expect("RwLock cannot be locked")
    }
}

impl StorageTrait for Storage {
    fn read(&self, offset: u64, dst: &mut [u8]) {
        // let slice = &self.mem.borrow()[offset as usize..offset as usize + dst.len()];
        // dst.copy_from_slice(slice);
        let mem = self.mem.read().unwrap();
        let slice = &mem[offset as usize..offset as usize + dst.len()];
        dst.copy_from_slice(slice);
    }

    fn write(&self, offset: u64, src: &[u8]) {
        // let slice = &mut self.mem.borrow_mut()[offset as usize..offset as usize + src.len()];
        // slice.copy_from_slice(src);
        let mut mem = self.mem.write().unwrap();
        let slice = &mut mem[offset as usize..offset as usize + src.len()];
        slice.copy_from_slice(src);
    }

    fn sector_byte_size(&self) -> usize {
        todo!()
    }
}

struct VectorReader {
    vectors: Vec<Vec<f32>>
}

impl OriginalVectorReaderTrait<f32> for VectorReader {
    fn read(&self, index: &usize) -> Result<Vec<f32>> {
        Ok(self.vectors[*index].clone())
    }

    fn read_with_range(&mut self, _start: &usize, _end: &usize) -> Result<Vec<Vec<f32>>> {
        todo!()
    }

    fn get_num_vectors(&self) -> usize {
        self.vectors.len()
    }

    fn get_vector_dim(&self) -> usize {
        self.vectors[0].len()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Item {
    text: String,
    embeddings: Option<String>,
}
type Items = Vec<Item>;

#[wasm_bindgen]
pub struct Vectune {
    dim: usize,
    degree: usize,
    seed: u64,
}
#[wasm_bindgen]
impl Vectune {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {

        let dim = 384;
        let degree = 90;
        let seed: u64 = 0123456789;

        Self {
            dim,
            degree,
            seed
        }
    }

    pub fn build(&mut self, input: JsValue) -> Result<Vec<u8>, JsError> {
        let mut data_map = ICRBTree::new();
        let items: Items = serde_wasm_bindgen::from_value(input).map_err(|m| JsError::new(&m.to_string()))?;
        let vectors: Vec<Vec<f32>> = items.into_iter().enumerate().map(|(index, item)| {

            // Insert data to ic-rbtree
            let index: u32 = index.try_into().unwrap();
            let _ = data_map.insert(index, item.text.into_bytes());

            // Vectorize text if embeddings is null
            if let Some(embeddings) = item.embeddings {
                if let Ok(bytes) = base64::decode(embeddings) {
                    if let Ok(slice) = bytemuck::try_cast_slice(&bytes) {
                        let vector: Vec<f32> = slice.to_vec();
                        return vector;
                    }
                }
            }

            todo!()
        }).collect();
        let file_byte_size = ssd_vectune::utils::node_byte_size(self.dim) * vectors.len();
        let storage = Storage::new(file_byte_size.try_into().unwrap());
        let graph_on_storage = GraphStore::new(vectors.len(), self.dim, self.degree, storage);
        let vector_reader = VectorReader {
            vectors
        };

        let (medoid_index, backlinks) = ssd_vectune::single_index::single_index(&vector_reader, &graph_on_storage, self.seed);

        Ok(Vectune::serialize(data_map, graph_on_storage, backlinks, medoid_index))
    }

    fn serialize(data_map: ICRBTree, graph_on_storage: GraphStore<Storage>, backlinks: Vec<Vec<u32>>, medoid_index: u32) -> Vec<u8> {

        // data map
        let serialized_data_map = data_map.into_memory();
        // graph
        let serialized_graph_store = graph_on_storage.into_storage().into_memory();
        // backlinks
        let serialized_backlinks_map: Vec<u8> = ICRBTree::build_from_vec_u8(backlinks.into_iter().map(|links| bytemuck::cast_slice(&links).to_vec()).collect()).into_memory();

        // header
        /*
        |data_map_len: u64|graph_store_len: u64|backlinks_map_len: u64|medoid_index: u32|
        */
        let mut bytes = vec![];
        bytes.extend((serialized_data_map.len() as u64).to_le_bytes());
        bytes.extend((serialized_graph_store.len() as u64).to_le_bytes());
        bytes.extend((serialized_backlinks_map.len() as u64).to_le_bytes());
        bytes.extend(medoid_index.to_le_bytes());
        bytes.extend(serialized_data_map);
        bytes.extend(serialized_graph_store);
        bytes.extend(serialized_backlinks_map);



        bytes
    }
}

fn main() {
  console_error_panic_hook::set_once();
}