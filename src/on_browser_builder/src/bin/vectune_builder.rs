use std::{
    cell::RefCell,
    sync::{Arc, RwLock},
};

use log::debug;

use bitvec::prelude::*;

use vectune::PointInterface;
// use vectune::PointInterface;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test_configure;
wasm_bindgen_test_configure!(run_in_browser);

use anyhow::{Error, Result};
use ic_stable_structures::{BTreeMap as StableBTreeMap, Memory};
use ssd_vectune::{
    graph_store::GraphStore,
    storage::StorageTrait,
};

// candle lib
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::{PaddingParams, Tokenizer};

use base64::{engine::general_purpose, Engine as Base64Engine};

use on_browser_builder::points::Point;
// use on_browser_builder::console_log;

// const INSTANCE_BYTES: &[u8] = include_bytes!("models/model.safetensors");
const WEIGHTS: &[u8] = include_bytes!("../../models/model.safetensors");
const CONFIG: &[u8] = include_bytes!("../../models/config.json");
const TOKENIZER: &[u8] = include_bytes!("../../models/tokenizer.json");

const WASM_PAGE_SIZE: u64 = 65536;

// #[cfg_attr(not(test), wasm_bindgen)]
// #[wasm_bindgen]
pub struct EmbeddingModel {
    bert: BertModel,
    tokenizer: Tokenizer,
}
// #[wasm_bindgen]
impl EmbeddingModel {
    // #[wasm_bindgen(constructor)]
    pub fn new() -> Result<EmbeddingModel> {
        console_error_panic_hook::set_once();
        // console_log!("loading model");
        let device = &Device::Cpu;
        debug!("device: {:?}", device);
        let vb = VarBuilder::from_slice_safetensors(WEIGHTS, DType::F32, device)?;
        debug!("load model weights");
        let config: Config = serde_json::from_slice(CONFIG)?;
        // let tokenizer =
        //     Tokenizer::from_bytes(TOKENIZER).map_err(|m| JsError::new(&m.to_string()))?;
        debug!("load config");
        let tokenizer = Tokenizer::from_bytes(TOKENIZER).map_err(|m| Error::msg(m.to_string()))?;
        debug!("load tokenizer");
        let bert = BertModel::load(vb, &config)?;
        debug!("load model");

        Ok(Self { bert, tokenizer })
    }

    // pub fn get_embeddings(&mut self, input: JsValue) -> Result<JsValue, JsError> {
    //     let sentences: Vec<String> =
    //         serde_wasm_bindgen::from_value(input).map_err(|m| JsError::new(&m.to_string()))?;
    //     let embeddings = self._get_embeddings(&sentences, true)?;

    //     Ok(serde_wasm_bindgen::to_value(&embeddings)?)
    // }

    fn _get_embeddings(
        &mut self,
        sentences: &Vec<String>,
        normalize_embeddings: bool,
    ) -> Result<Vec<Vec<f32>>> {
        let device = &Device::Cpu;
        // set padding setting
        if let Some(pp) = self.tokenizer.get_padding_mut() {
            pp.strategy = tokenizers::PaddingStrategy::BatchLongest
        } else {
            let pp = PaddingParams {
                strategy: tokenizers::PaddingStrategy::BatchLongest,
                ..Default::default()
            };
            self.tokenizer.with_padding(Some(pp));
        }
        // set truncation setting TruncationParams
        let _ = self
            .tokenizer
            .with_truncation(Some(tokenizers::TruncationParams::default()));

        let tokens = self
            .tokenizer
            .encode_batch(sentences.to_vec(), true)
            .map_err(|m| Error::msg(m.to_string()))?;

        let token_ids: Vec<Tensor> = tokens
            .iter()
            .map(|tokens| {
                let tokens = tokens.get_ids().to_vec();
                Tensor::new(tokens.as_slice(), device)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let token_ids = Tensor::stack(&token_ids, 0)?;
        let token_type_ids = token_ids.zeros_like()?;
        // console_log!("running inference on batch {:?}", token_ids.shape());
        let embeddings = self.bert.forward(&token_ids, &token_type_ids)?;
        // console_log!("generated embeddings {:?}", embeddings.shape());
        // Apply some avg-pooling by taking the mean embedding value for all tokens (including padding)
        let (_n_sentence, n_tokens, _hidden_size) = embeddings.dims3()?;
        let embeddings = (embeddings.sum(1)? / (n_tokens as f64))?;
        let embeddings = if normalize_embeddings {
            embeddings.broadcast_div(&embeddings.sqr()?.sum_keepdim(1)?.sqrt()?)?
        } else {
            embeddings
        };
        let embeddings_data = embeddings.to_vec2()?;
        Ok(embeddings_data)
    }
}

struct ICMemory {
    mem: RefCell<Vec<u8>>,
}

impl Memory for ICMemory {
    fn size(&self) -> u64 {
        let mem_len = self.mem.borrow().len() as u64;
        return (mem_len + WASM_PAGE_SIZE - 1) / WASM_PAGE_SIZE;
    }

    fn grow(&self, pages: u64) -> i64 {
        let current_page_size = self.size();
        if (current_page_size + pages) * WASM_PAGE_SIZE > (usize::MAX) as u64 {
            return -1;
        }

        let prev_page_size = current_page_size;
        self.mem
            .borrow_mut()
            .extend(vec![0; (pages * WASM_PAGE_SIZE) as usize]);

        // println!("{}, {}", prev_page_size, self.size());
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
    ic_rbtree: StableBTreeMap<u32, Vec<u8>, ICMemory>,
}
impl ICRBTree {
    pub fn new() -> Self {
        let ic_rbtree = StableBTreeMap::new(ICMemory {
            mem: RefCell::new(vec![]),
        });
        Self { ic_rbtree }
    }

    pub fn insert(&mut self, key: u32, value: Vec<u8>) -> Option<Vec<u8>> {
        self.ic_rbtree.insert(key, value)
    }

    pub fn get(&mut self, key: u32) -> Option<Vec<u8>> {
        self.ic_rbtree.get(&key)
    }

    pub fn build_from_vec_u8(items: Vec<Vec<u8>>) -> Self {
        let mut ic_rbtree = StableBTreeMap::new(ICMemory {
            mem: RefCell::new(vec![]),
        });
        for (index, item) in items.into_iter().enumerate() {
            let _ = ic_rbtree.insert(index.try_into().unwrap(), item);
        }

        Self { ic_rbtree }
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
        let mem = self.mem.read().unwrap();
        let slice = &mem[offset as usize..offset as usize + dst.len()];
        dst.copy_from_slice(slice);
    }

    fn write(&self, offset: u64, src: &[u8]) {
        let mut mem = self.mem.write().unwrap();
        let slice = &mut mem[offset as usize..offset as usize + src.len()];
        slice.copy_from_slice(src);
    }

    fn sector_byte_size(&self) -> usize {
        0
    }
}


#[derive(serde::Serialize, serde::Deserialize)]
struct Item {
    sentence: String,
    embedding: Option<String>,
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
        Vectune::_new()
    }

    fn _new() -> Self {
        let dim = 384;
        let degree = 90;
        let seed: u64 = 0123456789;

        Self { dim, degree, seed }
    }

    pub fn build(&mut self, input: JsValue) -> Result<JsValue, JsError> {
        let items: Items =
            serde_wasm_bindgen::from_value(input).map_err(|m| JsError::new(&m.to_string()))?;

        Ok(serde_wasm_bindgen::to_value(
            &self
                ._build(items)
                .map_err(|m| JsError::new(&m.to_string()))?,
        )?)
    }

    fn _build(&mut self, items: Items) -> Result<Vec<u8>> {
        let mut bert = EmbeddingModel::new()?;
        let mut data_map = ICRBTree::new();

        let vectors: Vec<Vec<f32>> = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                // Insert data to ic-rbtree
                let index: u32 = index.try_into().unwrap();
                let _ = data_map.insert(index, item.sentence.clone().into_bytes());

                // Vectorize text if embeddings is null
                if let Some(embeddings) = item.embedding {
                    if let Ok(bytes) = general_purpose::STANDARD.decode(embeddings) {
                        if let Ok(slice) = bytemuck::try_cast_slice(&bytes) {
                            let vector: Vec<f32> = slice.to_vec();
                            return Ok(vector);
                        }
                    }
                }
                println!("index: {}", index);

                // todo: use batch embedding
                match bert._get_embeddings(&vec![item.sentence], true) {
                    Ok(embeddings) => Ok(embeddings[0].clone()),
                    Err(err) => Err(err),
                }

            })
            .collect::<Result<Vec<Vec<f32>>, _>>()?;


        println!("finish embedding");

        let file_byte_size = ssd_vectune::utils::node_byte_size(self.dim) * vectors.len();
        let storage = Storage::new(file_byte_size.try_into().unwrap());
        let graph_on_storage = GraphStore::new(vectors.len(), self.dim, self.degree, storage);
        // let vector_reader = VectorReader { vectors };

        /*

        wip:
        vectorsを呼び出し元のjsにJSONとして渡す。
        */

        // let (medoid_index, backlinks) =
        //     ssd_vectune::single_index::single_index(&vector_reader, &graph_on_storage, self.seed);
        let (indexed_points, medoid_index, backlinks): (Vec<(Point, Vec<u32>)>, u32, Vec<Vec<u32>>) =
            vectune::Builder::default()
                .set_seed(self.seed)
                .set_a(2.0)
                // .set_r(3)
                // .progress(ProgressBar::new(1000))
                .build(vectors.into_iter().map(|vector| Point::from_f32_vec(vector)).collect());

        indexed_points
                .into_iter()
                .enumerate()
                .for_each(|(node_id, (point, edges))| {
                    graph_on_storage
                        .write_node(&(node_id as u32), &point.to_f32_vec(), &edges)
                        .unwrap();
                });

        Ok(Vectune::serialize(
            data_map,
            graph_on_storage,
            backlinks,
            medoid_index,
        ))
    }

    fn serialize(
        data_map: ICRBTree,
        graph_on_storage: GraphStore<Storage>,
        backlinks: Vec<Vec<u32>>,
        medoid_index: u32,
    ) -> Vec<u8> {
        let num_vectors = graph_on_storage.num_vectors() as u32;
        let vector_dim = graph_on_storage.vector_dim() as u32;
        let edge_degrees = graph_on_storage.edge_max_degree() as u32;

        // data map
        let serialized_data_map = data_map.into_memory();
        // graph
        let serialized_graph_store = graph_on_storage.into_storage().into_memory();
        // backlinks
        let serialized_backlinks_map: Vec<u8> = ICRBTree::build_from_vec_u8(
            backlinks
                .into_iter()
                .map(|links| bytemuck::cast_slice(&links).to_vec())
                .collect(),
        )
        .into_memory();

       
        /*
        header
        |data_map_len: u64|graph_store_len: u64|backlinks_map_len: u64|medoid_index: u32|num_vectors: u32|vector_dim: u32|edge_degrees: u32|

        body
        |serialized_data_map|serialized_graph_store|serialized_backlinks_map|
        */
        
        let mut bytes = vec![];
        // header
        bytes.extend((serialized_data_map.len() as u64).to_le_bytes()); // 0-8
        bytes.extend((serialized_graph_store.len() as u64).to_le_bytes()); // 8-16
        bytes.extend((serialized_backlinks_map.len() as u64).to_le_bytes()); // 16-24
        bytes.extend(medoid_index.to_le_bytes()); // 24-28
        bytes.extend(num_vectors.to_le_bytes()); // 28-32
        bytes.extend(vector_dim.to_le_bytes()); // 32-36
        bytes.extend(edge_degrees.to_le_bytes()); // 36-40
        // body
        bytes.extend(serialized_data_map);
        bytes.extend(serialized_graph_store);
        bytes.extend(serialized_backlinks_map);

        bytes
    }
}


#[wasm_bindgen]
pub fn extract_false_indices_from_serialized_bitvec(input: JsValue) -> Result<JsValue, JsError> {
    let bytes: Vec<u8> = serde_wasm_bindgen::from_value(input).map_err(|m| JsError::new(&m.to_string()))?;
    let bitvec: BitVec<u8, Lsb0> = bincode::deserialize(&bytes)?;

    let indices: Vec<usize> = bitvec
        .into_iter()
        .enumerate()
        .filter_map(|(index, bit)| if !bit { Some(index) } else { None })
        .collect();
    Ok(serde_wasm_bindgen::to_value(&indices)?)
}

fn main() {
    console_error_panic_hook::set_once();
}

#[cfg(test)]
mod tests {

    use super::*;
    use rand::Rng;

    fn generate_random_string(length: usize) -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                                abcdefghijklmnopqrstuvwxyz\
                                0123456789";
        let mut rng = rand::thread_rng();

        let random_string: String = (0..length)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();

        random_string
    }

    #[test]
    fn ic_rbtree() {
        let mut map = ICRBTree::new();

        for i in 0..1000 as u32 {
            map.insert(i, generate_random_string(999).into_bytes());
        }

        let memory = map.into_memory();

        let map = StableBTreeMap::<u32, String, ICMemory>::init(ICMemory {
            mem: RefCell::new(memory),
        });

        for i in 0..1000 as u32 {
            assert!(map.get(&i).is_some())
        }
    }

    // #[wasm_bindgen_test]
    #[test]
    fn build() {
        let mut vectune = Vectune::new();
        let items: Vec<Item> = (0..10).into_iter().map(|_| Item {sentence: generate_random_string(100), embedding: None}).collect();
        let res = vectune._build(items);
        // println!("{:?}", res);
        assert!(res.is_ok());

        // match EmbeddingModel::new() {
        //     Ok(_) => {}
        //     Err(err) => {
        //         println!("{:?}", err);
        //         assert!(false);
        //     }
        // }
    }
}