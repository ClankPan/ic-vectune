use ic_cdk::{query, trap, update};
use ic_stable_structures::memory_manager::MemoryId;
use ic_stable_structures::Memory;
// use ssd_vectune::graph::Graph;
use ssd_vectune::graph_store::GraphStore;
use url::Url;
use vectune::GraphInterface;
use vectune::{InsertType, PointInterface};

use crate::{auth::*, consts::*, graph::*, thread_locals::*, types::*};

use crate::point::Point;

#[update(guard = "is_owner")]
fn batch_pool(
    batch_delete: Vec<u32>,
    batch_modify: Vec<(u32, String)>,
    batch_insert: Vec<(Vec<f32>, String)>,
) -> Vec<u32> {
    let mut graph = load_graph();


    let mut new_ids: Vec<_> = vec![];
    BATCH_POOL.with(|list| {
        let mut list = list.borrow_mut();
        batch_delete.into_iter().for_each(|id| {
            if list.insert(id, OptType::Delete).is_some() {
                panic!("fail insertion to bach list, delete opt")
            };
        });
        batch_modify.into_iter().for_each(|(id, metadata)| {
            if list.insert(id, OptType::Modify(metadata)).is_some() {
                panic!("fail insertion to bach list, modify opt");
            }
        });
        batch_insert.into_iter().for_each(|(embedding, metadata)| {
            let new_id = graph.alloc(Point::from_f32_vec(embedding));
            // let (embedding, _): (Point, Vec<_>) = graph.get(&new_id);
            // ic_cdk::println!("in batch_pool, embedding: {:?}", embedding);
            new_ids.push(new_id);
            if list.insert(new_id, OptType::Insert(metadata)).is_some() {
                panic!("fail insertion to bach list, insert opt")
            }
        });
    });

    ic_cdk::println!("enquened {} items", new_ids.len());

    new_ids
}


#[update(guard = "is_owner")]
async fn next_batch_step(max_iter: u64) -> Option<u64> {
    let mut graph = load_graph();

    let mut maximum_num_instructions: u64 = 0;

    const EXECUTION_LIMIT: u64 = 40000000000;

    
    let mut iter_count = 0;

    while let (Some((id, item)), list_len) = BATCH_POOL.with(|list| {let mut list = list.borrow_mut(); (list.pop_first(), list.len())}) {
        match item {
            OptType::Delete => {
                // ic_cdk::println!("delete id : {id}");
                graph.suspect(id)
            },
            OptType::Modify(new_metadata) => {
                SOURCE_DATA.with(|data_map| {
                    let mut data_map = data_map.borrow_mut();
                    data_map.insert(id, new_metadata);
                });
            }
            OptType::Insert(new_metadata) => {
                SOURCE_DATA.with(|data_map| {
                    let mut data_map = data_map.borrow_mut();
                    data_map.insert(id, new_metadata);
                });
                let start = ic_cdk::api::instruction_counter();
                vectune::insert(&mut graph, InsertType::<Point>::Id(id));
                let current = ic_cdk::api::instruction_counter();

                /*
                Note:
                insertの前後で、命令数を見て、もう一度やったらlimitになる場合にcommit-callをする。
                */
                maximum_num_instructions = std::cmp::max(maximum_num_instructions, current - start);

                if current + (maximum_num_instructions + maximum_num_instructions / 2) >  EXECUTION_LIMIT {
                    ic_cdk::println!("list_len: {list_len}, current instructions: {current}, maximum_num_instructions: {maximum_num_instructions}");
                    iter_count += 1;

                    if iter_count >= max_iter {
                        return  Some(list_len)
                    } else {
                        let _: Result<(), _> = ic_cdk::call(ic_cdk::id(), "commit", ()).await;
                    }
                    
                }

            }
        }

    }
    
    let _: Result<(), _> = ic_cdk::call(ic_cdk::id(), "commit", ()).await;
    vectune::delete::<Point, Graph<Storage>>(&mut graph);

    None

}

pub fn load_graph() -> Graph<Storage> {
    let storage_mem =
        MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(VAMANA_GRAPH_MEMORY_ID)));
    let storage = Storage { storage_mem };

    let graph_on_storage = GraphStore::load(
        // metadata.num_vectors as usize,
        // metadata.vector_dim as usize,
        // metadata.edge_degrees as usize,
        storage,
    );

    let graph = Graph::new(graph_on_storage);

    graph
}

#[query]
fn debug_get_batch_pool_len() -> u64 {
    BATCH_POOL.with(|map| map.borrow().len())
}

#[query]
fn debug_get_edges(id: u32) -> Vec<u32> {
    let mut graph = load_graph();
    let (_, edges): (Point, Vec<u32>) = graph.get(&id);
    edges
}

#[query]
fn debug_get_batch_pool() -> Vec<(u32, OptType)> {
    BATCH_POOL.with(|list| {
        let list = list.borrow();
        list.iter().collect()
    })
}

#[query]
fn debug_get_item(index: u32) -> String {
    let data = SOURCE_DATA.with(|map| map.borrow().get(&index).unwrap());
    data
}
#[query]
fn debug_get_backlinks(index: u32) -> Vec<u32> {
    let data = BACKLINKS_MAP.with(|map| map.borrow().get(&index).unwrap());
    data.0.into_iter().collect()
}

#[query]
fn debug_get_keys() -> Vec<(u32, String)> {
    SOURCE_DATA.with(|map| map.borrow().iter().collect())
}

// #[query]
// fn debug_get_backlinks() -> Vec<(u32, Vec<u32>)> {
//     BACKLINKS_MAP.with(|map| map.borrow().iter().map(|(k, v)| (k, v.0.into_iter().collect())).collect())
// }

#[query]
fn debug_get_raw_backlinks() -> Vec<u8> {
    let storage_mem = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(BACKLINKS_MEMORY_ID)));
    let mut buffer = vec![0u8; 1024];
    storage_mem.read(0, &mut buffer);

    buffer
}

#[query]
fn debug_get_raw_datamap() -> Vec<u8> {
    let storage_mem = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(DATA_MAP_MEMORY_ID)));
    let mut buffer = vec![0u8; WASM_PAGE_SIZE as usize];
    storage_mem.read(0, &mut buffer);

    buffer
}

#[query]
fn search(query_vector: Vec<f32>, top_k: u64, size_l: u64) -> Vec<SearchResponse> {
    let _start = ic_cdk::api::time();
    ic_cdk::println!("time: {}", ic_cdk::api::time());

    assert!(top_k <= size_l);

    METADATA.with(|metadata| {
        let metadata = metadata.borrow();
        let Metadata::Running(_metadata) = &*metadata.get() else {
            trap("Metadata is not Running")
        };

        // let storage_mem =
        //     MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(VAMANA_GRAPH_MEMORY_ID)));
        // let storage = Storage {
        //     storage_mem,
        // };

        // let graph_on_storage = GraphStore::load(
        //     // metadata.num_vectors as usize,
        //     // metadata.vector_dim as usize,
        //     // metadata.edge_degrees as usize,
        //     storage,
        // );

        // let mut graph = Graph::new(graph_on_storage);

        let mut graph = load_graph();

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

        ic_cdk::println!("k-ann: {:?}", k_ann);

        // todo!("get data from map");

        k_ann
            .into_iter()
            .map(|(dist, index)| {
                let data = SOURCE_DATA.with(|map| map.borrow().get(&index).unwrap());
                SearchResponse {
                    similarity: 1.0 - dist,
                    data,
                }
            })
            .collect()
    })
}

#[query]
fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}

#[query]
fn http_request(req: HttpRequest) -> HttpResponse {
    // ic_cdk::println!("{:?}", req);
    let url = Url::parse(&format!("https://localhost{}", req.url)).unwrap();
    let path = url.path();

    let headers = vec![
        (
            String::from("Access-Control-Allow-Origin"),
            String::from("*"),
        ),
        (
            String::from("Access-Control-Allow-Methods"),
            String::from("GET, POST, HEAD, OPTIONS"),
        ),
        (
            String::from("Access-Control-Allow-Headers"),
            String::from("Content-Type"),
        ),
    ];

    if req.method == "OPTIONS" {
        ic_cdk::println!(" req.method == OPTIONS");
        return HttpResponse {
            status_code: 200,
            headers,
            body: vec![],
        };
    }

    if path == "/" {
        ic_cdk::println!("path != /");
        return HttpResponse {
            status_code: 200,
            headers,
            body: b"Hello!:".to_vec(),
        };
    }

    if path != "/search" {
        ic_cdk::println!("path != /search");
        return HttpResponse {
            status_code: 404,
            headers,
            body: b"404 Not found :".to_vec(),
        };
    }

    if req.body.len() == 0 {
        ic_cdk::println!("req.body.len() == 0");
        return HttpResponse {
            status_code: 400,
            headers,
            body: b"Body must be 384 long float32 array".to_vec(),
        };
    }
    // ic_cdk::println!("{:?}",  String::from_utf8(req.body.clone()));
    let (status_code, body) = match std::str::from_utf8(&req.body) {
        Ok(utf8_str) => match serde_json::from_str::<(Vec<f32>, u64, u64)>(&utf8_str) {
            Ok((query, size_top_k, size_l)) => {
                let k_ann = search(query, size_top_k, size_l);
                let status_code = 200;
                let body = serde_json::to_string(&k_ann).unwrap().into_bytes();
                (status_code, body)
            }
            Err(err) => {
                ic_cdk::println!("err in parsing json");
                let status_code = 400;
                let body =
                    serde_json::to_string(&format!("JSON parsing error: {}", err.to_string()))
                        .unwrap()
                        .into_bytes();
                (status_code, body)
            }
        },
        Err(err) => {
            ic_cdk::println!("err in parsing utf8");
            let status_code = 400;
            let body = serde_json::to_string(&format!("Urf8 parsing error: {}", err.to_string()))
                .unwrap()
                .into_bytes();
            (status_code, body)
        }
    };

    HttpResponse {
        status_code,
        headers,
        body,
    }
}
