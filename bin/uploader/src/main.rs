use std::{fs::File, sync::Arc, time::Instant};

use anyhow::Result;
use bitvec::prelude::*;
use bytesize::KIB;
use candid::{Decode, Encode};
use ic_agent::{agent::http_transport::reqwest_transport::reqwest::Response, export::Principal, identity, Agent};
use memmap2::Mmap;
use ssd_vectune::{graph::GraphMetadata, original_vector_reader::{read_ivecs, OriginalVectorReader, OriginalVectorReaderTrait}};
use tokio;
use rand::{thread_rng, Rng};
use futures::stream::{self, StreamExt};


//  cargo run --release --bin uploader -- upload  <graph path> <graph metadata path> <canister id> --name clankpan

enum UP {
    Done,
    Continue(BitVec<u8>),
}

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
    /// Executes the build process, including merge-index and gorder
    Upload {
        #[arg(long)]
        ic: bool,
    
        #[arg(long, default_value = "default")]
        name: String,
    
        #[arg(long, default_value = "1024")]
        chunk_kib_size: usize,
    
        source_data_path: String,
        graph_metadata_path: String,
        target_canister_id: String,
    },
    Search {
        #[arg(long)]
        ic: bool,
        #[arg(long)]
        simd: bool,
        query_path: String,
        ground_truth_path: String,
        target_canister_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {

    let cli = Cli::parse();

    match cli.command {
        Commands::Upload { ic, name, chunk_kib_size, source_data_path, graph_metadata_path, target_canister_id } => {

            let agent = Arc::new(get_agent(&name, ic).await?);
            let target_canister_id = Principal::from_text(target_canister_id)?;
            let chunk_byte_size = chunk_kib_size * KIB as usize;
        
            let chunk_reader = Arc::new(ChunkReader::new(&source_data_path, chunk_byte_size)?);
            let graph_metadata = GraphMetadata::load(&graph_metadata_path).unwrap();
        
            let num_chunks = (chunk_reader.file_size() + chunk_byte_size - 1) / chunk_byte_size;
        
            assert!(chunk_reader.file_size() <= num_chunks*chunk_byte_size);
        
            println!("graph_metadata.edge_degrees {}", graph_metadata.edge_degrees);
        
        
            println!("calling status_code..");
            match call_status_code(&agent, target_canister_id).await? {
        
                0 => {
                    println!("calling initialize..");
                    call_initialize(
                        &agent,
                        target_canister_id,
                        num_chunks as u64,
                        chunk_byte_size as u64,
                        graph_metadata.medoid_node_index,
                        graph_metadata.sector_byte_size as u64,
                        graph_metadata.num_vectors as u64,
                        graph_metadata.vector_dim as u64,
                        // graph_metadata.edge_degrees as u64,
                        90
                    )
                    .await?;
                },
                1 => {
                    println!("skip call_initialize")
                },
                2 => {
                    todo!()
                },
                _ => todo!()
            }
        
            println!("start loop");
        
            while let UP::Continue(uploaded_chunks) = {
                println!("calling missing_chunks...");
                let uploaded_chunks = get_missing_chunks(&agent, target_canister_id).await?;
                let missing_counts = uploaded_chunks.iter().filter(|bit| !**bit).count();
        
                assert!(chunk_reader.file_size() <= uploaded_chunks.len() * chunk_byte_size);
        
        
                match missing_counts {
                    0 => {
                        UP::Done
                    },
                    _ => {
                        println!("missing_counts: {missing_counts}");
                        UP::Continue(uploaded_chunks)
                    },
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
                        )
                        .await;
                        println!("chunk_index: {chunk_index}/{uploaded_chunks_len}");
                        match response {
                            Ok(_) => {},
                            Err(err) => {
                                println!("{:?}", err);
                            }
                        }
                    })
                });
        
                let _results: Vec<_> = task_stream.buffered(10).collect().await;
            }
        
        
            Ok(())
        },
        Commands::Search { ic, simd, query_path, ground_truth_path, target_canister_id } => {

            let target_canister_id = Principal::from_text(target_canister_id)?;

            let agent = Arc::new(get_anonymous_agent(ic).await?);

            let query_vector_reader = OriginalVectorReader::new(&query_path)?;
            let groundtruth: Vec<Vec<u32>> = read_ivecs(&ground_truth_path).unwrap();
        
            let query_iter = 100;
            let mut total_time = 0;
            // let mut rng = thread_rng();
        
            let mut hit_sum = 0;
            for query_index in 0..query_iter {
                // let random_query_index  = rng.gen_range(0..query_vector_reader.get_num_vectors());
                // let query_vector: Vec<f32> = query_vector_reader.read(&random_query_index).unwrap();
                let query_vector: Vec<f32> = query_vector_reader.read(&query_index).unwrap();
                println!("query_index {query_index}");
        
                let start = Instant::now();
        
                let k_ann = call_search(&agent, target_canister_id, &query_vector, simd).await?;
        
                let t = start.elapsed().as_millis();
                total_time += t;
        
                let result_top_5: Vec<u32> = k_ann.into_iter().map(|(_, i)| i).collect();
                let top5_groundtruth = &groundtruth[query_index][0..5];
                println!("{:?}\n{:?}", top5_groundtruth, result_top_5);
                let mut hit = 0;
                for res in result_top_5 {
                    if top5_groundtruth.contains(&res) {
                        hit += 1;
                    }
                }
                hit_sum += hit;
        
                println!("hit: {}/{}\n", hit, top5_groundtruth.len());
        
            }

            println!("average query-time:  {} ms", total_time as f32 / query_iter as f32);
            println!("average recall-rate: {} %", (hit_sum as f32 / (query_iter * 5) as f32) * 100.0);

            Ok(())
        },
    }

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

async fn get_anonymous_agent(is_ic: bool) -> Result<Agent> {
    let host = if is_ic {
        "https://ic0.app"
    } else {
        "http://127.0.0.1:4943"
    };
    let agent = Agent::builder()
        .with_url(host)
        .build()?;

    if !is_ic {
        agent.fetch_root_key().await.unwrap();
        }

    Ok(agent)
}

async fn call_search(
    agent: &Agent,
    target_canister_id: Principal,
    query_vector: &Vec<f32>,
    simd: bool,
) -> Result<Vec<(f32, u32)>> {
    let method_name = if simd { "search_with_simd" } else { "search" };
    let top_k: u64 = 5;
    let size_l: u64 = 100;
    let response = agent
        .query(&target_canister_id, method_name)
        .with_arg(Encode!(query_vector, &top_k, &size_l)?)
        .call()
        .await?;
    let status_code = Decode!(&response, Vec<(f32, u32)>)?;

    Ok(status_code)
}

async fn call_status_code(
    agent: &Agent,
    target_canister_id: Principal,
) -> Result<u8> {
    let method_name = "status_code";
    let response = agent.query(&target_canister_id, method_name).with_arg(Encode!()?).call().await?;
    let status_code = Decode!(&response, u8)?;

    Ok(status_code)
}

async fn call_initialize(
    agent: &Agent,
    target_canister_id: Principal,

    num_chunks: u64,
    chunk_byte_size: u64,
    medoid_node_index: u32,
    sector_byte_size: u64,
    num_vectors: u64,
    vector_dim: u64,
    edge_degrees: u64,
) -> Result<()> {
    let method_name = "initialize";
    let _ = agent
        .update(&target_canister_id, method_name)
        .with_arg(Encode!(
            &num_chunks,
            &chunk_byte_size,
            &medoid_node_index,
            &sector_byte_size,
            &num_vectors,
            &vector_dim,
            &edge_degrees
        )?)
        .call_and_wait()
        .await?;
    Ok(())
}

async fn get_missing_chunks(
    agent: &Agent,
    target_canister_id: Principal,
) -> Result<BitVec<u8, Lsb0>> {

    let mut data = Vec::new();
    let mut index = 0;
    while let Some(bytes) = call_missing_chunks(agent, target_canister_id, index).await? {
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
) -> Result<Option<Vec<u8>>> {
    let method_name = "missing_chunks";
    let response = agent.query(&target_canister_id, method_name).with_arg(Encode!(&index)?).call().await?;
    let Some(uploaded_chunks) = Decode!(&response, Option<Vec<u8>>)?  else {return Ok(None)};

    Ok(Some(uploaded_chunks))
}

async fn call_upload_chunk(
    agent: &Agent,
    target_canister_id: Principal,
    arg: (Vec<u8>, u64),
) -> Result<()> {
    let method_name = "upload_chunk";
    let (chunk, index) = arg;
    let _ = agent
        .update(&target_canister_id, method_name)
        .with_arg(Encode!(&chunk, &index)?)
        .call_and_wait()
        .await?;
    Ok(())
}
