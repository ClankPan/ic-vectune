pub mod ic_memory;

use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

use anyhow::Result;
use bitvec::prelude::*;
use bytesize::KIB;
#[cfg(feature = "embedding-command")]
use candid::CandidType;
use candid::{Decode, Encode};
use futures::stream::{self, StreamExt};
use ic_agent::{export::Principal, identity, Agent};
use memmap2::Mmap;
// use serde::Deserialize;
use tokio;

use ic_memory::*;
//  cargo run --release --bin uploader -- upload  <graph path> <graph metadata path> <canister id> --name clankpan

struct ChunkReader {
    mmap: Mmap,
    chunk_byte_size: usize,
}

impl ChunkReader {
    pub fn new(path: &str, chunk_byte_size: usize) -> Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            mmap: unsafe { Mmap::map(&file)? },
            chunk_byte_size,
        })
    }

    pub fn read(&self, chunk_index: usize) -> Vec<u8> {
        let start = chunk_index * self.chunk_byte_size;
        let end = std::cmp::min(start + &self.chunk_byte_size, self.file_size());
        self.mmap[start..end].to_vec()
    }

    pub fn file_size(&self) -> usize {
        self.mmap.len()
    }
}

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    BuildIcStructure {
        #[arg(long)]
        backlinks_vec_path: String,
        #[arg(long)]
        metadata_vec_path: String,
        #[arg(long)]
        dir: String,
    },
    /// Executes the build process, including merge-index and gorder
    SetupUpload {
        #[arg(long)]
        ic: bool,

        #[arg(long, default_value = "default")]
        name: String,

        #[arg(long, default_value = "1024")]
        chunk_kib_size: usize,

        #[arg(long)]
        datamap_raw_memory_path: String,
        #[arg(long)]
        graph_raw_memory_path: String,
        #[arg(long)]
        backlinks_map_raw_memory_path: String,
        #[arg(long)]
        target_canister_id: String,
    },
    #[cfg(feature = "embedding-command")]
    Search {
        #[arg(long)]
        ic: bool,
        #[arg(long)]
        target_canister_id: String,
        #[arg(long)]
        query_text: String,
        #[arg(long)]
        model_dir: String,
    },

    Debug {
        #[arg(long)]
        ic: bool,
        #[arg(long)]
        target_canister_id: String,

        #[arg(long, default_value = "default")]
        name: String,
    },

    Batch {
        #[arg(long)]
        ic: bool,
        #[arg(long)]
        target_canister_id: String,

        #[arg(long, default_value = "default")]
        name: String,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Debug {
            ic,
            target_canister_id,
            name,
        } => {
            let agent = Arc::new(get_agent(&name, ic).await?);
            let target_canister_id = Principal::from_text(target_canister_id)?;

            let method_name = "debug_get_raw_datamap";
            let response = agent
                .query(&target_canister_id, method_name)
                .with_arg(Encode!()?)
                .call()
                .await?;
            let raw_memory = Decode!(&response, Vec<u8>)?;

            println!("raw_memory len: {}", raw_memory.len());

            let metadata_map = ICRBTree::<u32, String>::load_memory(raw_memory);

            println!("{:?}", metadata_map.get_items());

            Ok(())
        }
        Commands::BuildIcStructure {
            dir,
            backlinks_vec_path,
            metadata_vec_path,
        } => {
            let dir = Path::new(&dir);
            let backlinks_vec: Vec<Vec<u32>> =
                bincode::deserialize(&open_file_as_bytes(Path::new(&backlinks_vec_path))?)?;
            let metadata_vec: Vec<String> =
                bincode::deserialize(&open_file_as_bytes(Path::new(&metadata_vec_path))?)?;

            let backlinks_map = ICRBTree::<u32, Backlinks>::build_from_vec(
                backlinks_vec
                    .into_iter()
                    .enumerate()
                    .map(|(k, v)| (k as u32, Backlinks(v.into_iter().collect())))
                    .collect(),
            );
            let metadata_map = ICRBTree::<u32, String>::build_from_vec(
                metadata_vec
                    .into_iter()
                    .enumerate()
                    .map(|(k, v)| (k as u32, v))
                    .collect(),
            );

            // println!("{:?}", backlinks_map.get_items());
            // println!("{:?}", metadata_map.get_items());

            let backlinks_map_raw_memory = backlinks_map.into_memory();
            let metadata_map_raw_memory = metadata_map.into_memory();

            // println!("{:?}", backlinks_map_raw_memory);

            File::create(dir.join(format!("backlinks_map_raw_memory.bin")))?
                .write_all(&backlinks_map_raw_memory)?;
            File::create(dir.join(format!("metadata_map_raw_memory.bin")))?
                .write_all(&metadata_map_raw_memory)?;

            // let backlinks_map = ICRBTree::<u32, Backlinks>::load_memory(backlinks_map_raw_memory);
            // let metadata_map =  ICRBTree::<u32, String>::load_memory(metadata_map_raw_memory);

            // println!("{:?}", backlinks_map.get_items());
            // println!("{:?}", metadata_map.get_items());

            Ok(())
        }
        Commands::SetupUpload {
            ic,
            name,
            chunk_kib_size,
            target_canister_id,

            datamap_raw_memory_path,
            graph_raw_memory_path,
            backlinks_map_raw_memory_path,
        } => {
            let agent = Arc::new(get_agent(&name, ic).await?);
            let target_canister_id = Principal::from_text(target_canister_id)?;
            let chunk_byte_size = chunk_kib_size * KIB as usize;

            let source_chunk_reader =
                Arc::new(ChunkReader::new(&datamap_raw_memory_path, chunk_byte_size)?);
            let graph_chunk_reader =
                Arc::new(ChunkReader::new(&graph_raw_memory_path, chunk_byte_size)?);
            let backlinks_chunk_reader = Arc::new(ChunkReader::new(
                &backlinks_map_raw_memory_path,
                chunk_byte_size,
            )?);

            // let metadata_map =  ICRBTree::<u32, String>::load_memory(source_chunk_reader.read(0));
            // println!("{:?}", metadata_map.get_items());

            // println!("source_chunk_reader.read(0).len(): {}", source_chunk_reader.read(0).len());
            // let _metadata_map =  ICRBTree::<u32, String>::load_memory(source_chunk_reader.read(0));
            // println!("metadata_map {:?}", metadata_map.get_items());
            // println!("backlinks_chunk_reader.file_size(): {}", backlinks_chunk_reader.file_size());

            let num_datamap_chunks =
                num_chunks(source_chunk_reader.file_size(), chunk_byte_size) as u64;
            let num_graph_chunks =
                num_chunks(graph_chunk_reader.file_size(), chunk_byte_size) as u64;
            let num_backlinks_chunks =
                num_chunks(backlinks_chunk_reader.file_size(), chunk_byte_size) as u64;

            // todo: Match with ic-vectune/on_browser_builder.
            let db_key = String::from("aaaaaaa");

            println!("calling status_code..");
            match call_status_code(&agent, target_canister_id).await? {
                0 => {
                    println!("calling initialize..");
                    call_initialize(
                        &agent,
                        target_canister_id,
                        num_graph_chunks,
                        num_datamap_chunks,
                        num_backlinks_chunks,
                        chunk_byte_size as u64,
                        db_key,
                    )
                    .await?;
                }
                1 => {
                    println!("skip call_initialize")
                }
                2 => {
                    todo!()
                }
                _ => todo!(),
            }

            println!("start loop");

            upload_raw_memory(
                chunk_byte_size,
                agent.clone(),
                target_canister_id,
                ChunkType::DataMap,
                source_chunk_reader,
            )
            .await?;
            upload_raw_memory(
                chunk_byte_size,
                agent.clone(),
                target_canister_id,
                ChunkType::Graph,
                graph_chunk_reader,
            )
            .await?;
            upload_raw_memory(
                chunk_byte_size,
                agent.clone(),
                target_canister_id,
                ChunkType::BacklinksMap,
                backlinks_chunk_reader,
            )
            .await?;

            let _ = agent
                .update(&target_canister_id, "start_running")
                .with_arg(Encode!()?)
                .call_and_wait()
                .await?;

            Ok(())
        }
        #[cfg(feature = "embedding-command")]
        Commands::Search {
            ic,
            query_text,
            target_canister_id,
            model_dir,
        } => {
            let target_canister_id = Principal::from_text(target_canister_id)?;
            let agent = Arc::new(get_anonymous_agent(ic).await?);

            let model_dir = Path::new(&model_dir);
            let mut weights = Vec::new();
            File::open(model_dir.join("model.safetensors"))?.read_to_end(&mut weights)?;

            let mut config = Vec::new();
            File::open(model_dir.join("config.json"))?.read_to_end(&mut config)?;

            let mut tokenizer = Vec::new();
            File::open(model_dir.join("tokenizer.json"))?.read_to_end(&mut tokenizer)?;
            let model_params = ssd_vectune::embed::ModelPrams {
                weights,
                config,
                tokenizer,
            };

            let model = ssd_vectune::embed::EmbeddingModel::new(model_params)?;

            let query_vector = model.get_embeddings(&vec![query_text], true, "query").expect("msg");

            let k_ann = call_search(&agent, target_canister_id, &query_vector[0]).await?;

            println!("{:?}", k_ann);

            Ok(())
        }
        Commands::Batch { ic, target_canister_id, name } => {
            let agent = Arc::new(get_agent(&name, ic).await?);
            let target_canister_id = Principal::from_text(target_canister_id)?;

            while let Some(remain_batch_list_len) = call_next_batch_step(agent.clone(), target_canister_id).await? {
                println!("remain_batch_list_len: {remain_batch_list_len}");
            }

            Ok(())
        }
    }
}

async fn call_next_batch_step(
    agent: Arc<Agent>,
    target_canister_id: Principal,
) -> Result<Option<u64> >{
    let method_name = "next_batch_step";

    let max_iter: u64 = 5;

    let response = agent
        .update(&target_canister_id, method_name)
        .with_arg(Encode!(&max_iter)?)
        .call_and_wait()
        .await?;

     Ok(Decode!(&response, Option<u64>)?)
}

#[derive(candid::CandidType, candid::Deserialize, Clone, Copy)]
pub enum ChunkType {
    Graph,
    DataMap,
    BacklinksMap,
}
enum UploadLoop {
    Done,
    Continue(BitVec<u8>),
}

async fn upload_raw_memory(
    chunk_byte_size: usize,
    agent: Arc<Agent>,
    target_canister_id: Principal,
    chunk_type: ChunkType,
    chunk_reader: Arc<ChunkReader>,
) -> Result<()> {
    let chunk_reader = Arc::new(chunk_reader);

    println!(
        "chunk num: {}",
        num_chunks(chunk_reader.file_size(), chunk_byte_size)
    );

    while let UploadLoop::Continue(uploaded_chunks) = {
        println!("calling missing_chunks...");
        let uploaded_chunks = get_missing_chunks(&agent, target_canister_id, &chunk_type).await?;
        println!(
            "un_uploaded_chunks: {:?}",
            uploaded_chunks
                .iter()
                .enumerate()
                .filter_map(|(index, bit)| if !bit { Some(index) } else { None })
                .collect::<Vec<usize>>()
        );
        let missing_counts = uploaded_chunks.iter().filter(|bit| !**bit).count();

        assert!(chunk_reader.file_size() <= uploaded_chunks.len() * chunk_byte_size);

        match missing_counts {
            0 => UploadLoop::Done,
            _ => {
                println!("missing_counts: {missing_counts}");
                UploadLoop::Continue(uploaded_chunks)
            }
        }
    } {
        let uploaded_chunks_len = uploaded_chunks.len();

        let task_stream = stream::iter(
            uploaded_chunks
                .into_iter()
                .enumerate()
                .filter_map(|(index, bit)| if !bit { Some(index) } else { None }),
        )
        .map(|chunk_index| {
            let agent = agent.clone();
            let chunk_reader = chunk_reader.clone();
            tokio::spawn(async move {
                // Load chunk data from disk
                let chunk_byte_data = chunk_reader.read(chunk_index);

                // upload chunk into canister
                let response = call_upload_chunk(
                    &agent,
                    target_canister_id,
                    (chunk_byte_data, chunk_index as u64),
                    &chunk_type,
                )
                .await;
                println!("chunk_index: {chunk_index}/{uploaded_chunks_len}");
                match response {
                    Ok(_) => {}
                    Err(err) => {
                        println!("{:?}", err);
                    }
                }
            })
        });

        let _results: Vec<_> = task_stream.buffered(20).collect().await;
    }

    Ok(())
}

fn num_chunks(bytes_len: usize, chunk_byte_size: usize) -> usize {
    (bytes_len + chunk_byte_size - 1) / chunk_byte_size
}

async fn get_agent(name: &str, is_ic: bool) -> Result<Agent> {
    let mut path = dirs::home_dir().unwrap();
    path.push(format!(".config/dfx/identity/{name}/identity.pem"));
    let user_identity = identity::Secp256k1Identity::from_pem_file(path).unwrap();
    let host = if is_ic {
        "https://ic0.app"
    } else {
        "http://127.0.0.1:4943"
    };
    let agent = Agent::builder()
        .with_url(host)
        .with_identity(user_identity)
        .build()?;

    if !is_ic {
        agent.fetch_root_key().await.unwrap();
    }

    Ok(agent)
}

#[cfg(feature = "embedding-command")]
async fn get_anonymous_agent(is_ic: bool) -> Result<Agent> {
    let host = if is_ic {
        "https://ic0.app"
    } else {
        "http://127.0.0.1:4943"
    };
    let agent = Agent::builder().with_url(host).build()?;

    if !is_ic {
        agent.fetch_root_key().await.unwrap();
    }

    Ok(agent)
}
#[cfg(feature = "embedding-command")]
#[derive(CandidType, serde::Serialize, serde::Deserialize, Debug)]
pub struct SearchResponse {
    pub(crate) similarity: f32,
    pub(crate) data: String,
}
#[cfg(feature = "embedding-command")]
async fn call_search(
    agent: &Agent,
    target_canister_id: Principal,
    query_vector: &Vec<f32>,
) -> Result<Vec<SearchResponse>> {
    let method_name = "search";
    let top_k: u64 = 5;
    let size_l: u64 = 100;
    let response = agent
        .query(&target_canister_id, method_name)
        .with_arg(Encode!(query_vector, &top_k, &size_l)?)
        .call()
        .await?;
    let k_ann = Decode!(&response, Vec<SearchResponse>)?;

    Ok(k_ann)
}

async fn call_status_code(agent: &Agent, target_canister_id: Principal) -> Result<u8> {
    let method_name = "status_code";
    let response = agent
        .query(&target_canister_id, method_name)
        .with_arg(Encode!()?)
        .call()
        .await?;
    let status_code = Decode!(&response, u8)?;

    Ok(status_code)
}

async fn get_missing_chunks(
    agent: &Agent,
    target_canister_id: Principal,
    chunk_type: &ChunkType,
) -> Result<BitVec<u8, Lsb0>> {
    let mut data = Vec::new();
    let mut index = 0;
    while let Some(bytes) =
        call_missing_chunks(agent, target_canister_id, index, chunk_type).await?
    {
        println!("fetch");
        data.extend(bytes);
        index += 1;
    }

    let uploaded_chunks: BitVec<u8, Lsb0> = bincode::deserialize(&data).unwrap();

    Ok(uploaded_chunks)
}

async fn call_missing_chunks(
    agent: &Agent,
    target_canister_id: Principal,
    index: u64,
    chunk_type: &ChunkType,
) -> Result<Option<Vec<u8>>> {
    let method_name = "missing_chunks";
    let response = agent
        .query(&target_canister_id, method_name)
        .with_arg(Encode!(&index, chunk_type)?)
        .call()
        .await?;
    let Some(uploaded_chunks) = Decode!(&response, Option<Vec<u8>>)? else {
        return Ok(None);
    };

    Ok(Some(uploaded_chunks))
}

async fn call_upload_chunk(
    agent: &Agent,
    target_canister_id: Principal,
    arg: (Vec<u8>, u64),
    chunk_type: &ChunkType,
) -> Result<()> {
    let method_name = "upload_chunk";
    let (chunk, index) = arg;
    let _ = agent
        .update(&target_canister_id, method_name)
        .with_arg(Encode!(&chunk, &index, chunk_type)?)
        .call_and_wait()
        .await?;
    Ok(())
}

async fn call_initialize(
    agent: &Agent,
    target_canister_id: Principal,

    // num_chunks: u64,
    // chunk_byte_size: u64,
    // medoid_node_index: u32,
    // sector_byte_size: u64,
    // num_vectors: u64,
    // vector_dim: u64,
    // edge_degrees: u64,
    num_graph_chunks: u64,
    num_datamap_chunks: u64,
    num_backlinks_chunks: u64,
    chunk_byte_size: u64,

    db_key: String,
) -> Result<()> {
    let method_name = "start_loading";

    // This field is for compatibility and is ignored in the current version.
    let medoid_node_index: u32 = 0;
    let sector_byte_size: u64 = 0;
    let num_vectors: u64 = 0;
    let vector_dim: u64 = 0;
    let edge_degrees: u64 = 0;

    let _ = agent
        .update(&target_canister_id, method_name)
        .with_arg(Encode!(
            &num_graph_chunks,
            &num_datamap_chunks,
            &num_backlinks_chunks,
            &chunk_byte_size,
            &medoid_node_index,
            &sector_byte_size,
            &num_vectors,
            &vector_dim,
            &edge_degrees,
            &db_key
        )?)
        .call_and_wait()
        .await?;
    Ok(())
}

fn _open_file_as_string(path: &Path) -> Result<String> {
    let mut content = String::new();
    File::open(path)?.read_to_string(&mut content)?;
    Ok(content)
}

fn open_file_as_bytes(path: &Path) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;
    Ok(content)
}
