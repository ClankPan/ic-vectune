use ic_cdk::{query, trap};
use ic_stable_structures::memory_manager::MemoryId;
use ssd_vectune::graph::UnorderedGraph;
use ssd_vectune::graph_store::GraphStore;
use vectune::PointInterface;
use url::Url;

use crate::{consts::*, thread_locals::*, types::*};


use crate::point::Point;

/*


wip

距離はcos simに変更する

*/

#[query]
fn search(query_vector: Vec<f32>, top_k: u64, size_l: u64) -> Vec<SearchResponse> {

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

        // todo!("get data from map");

        k_ann.into_iter().map(|(dist, index)| {
            let data = SOURCE_DATA.with(|map| {
                let bytes = map.borrow().get(&index).unwrap();
                String::from_utf8(bytes).unwrap()
            });
            SearchResponse {
                similarity: 1.0-dist,
                data
            }
        }).collect()
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
    (String::from("Access-Control-Allow-Origin"), String::from("*")),
    (String::from("Access-Control-Allow-Methods"), String::from("GET, POST, HEAD, OPTIONS")),
    (String::from("Access-Control-Allow-Headers"), String::from("Content-Type")),
  ];

  if req.method == "OPTIONS" {
    ic_cdk::println!(" req.method == OPTIONS");
    return HttpResponse {
      status_code: 200,
      headers,
      body: vec![],
    }
  }

  if path == "/" {
    ic_cdk::println!("path != /");
    return HttpResponse {
      status_code: 200,
      headers,
      body: b"Hello!:".to_vec(),
    }
  }
  
  if path != "/search" {
    ic_cdk::println!("path != /search");
    return HttpResponse {
      status_code: 404,
      headers,
      body: b"404 Not found :".to_vec(),
    }
  }

  if req.body.len() == 0 {
    ic_cdk::println!("req.body.len() == 0");
    return HttpResponse {
      status_code: 400,
      headers,
      body: b"Body must be 384 long float32 array".to_vec(),
    }
  }
  // ic_cdk::println!("{:?}",  String::from_utf8(req.body.clone()));
  let (status_code, body) = match std::str::from_utf8(&req.body) {
    Ok(utf8_str) => {
      match serde_json::from_str::<(Vec<f32>, u64, u64)>(&utf8_str) {
        Ok((query, size_top_k, size_l)) => {
            let k_ann = search(query, size_top_k, size_l);
            let status_code = 200;
            let body = serde_json::to_string(&k_ann).unwrap().into_bytes();
            (status_code, body)
        },
        Err(err) => {
          ic_cdk::println!("err in parsing json");
          let status_code = 400;
          let body = serde_json::to_string(&format!("JSON parsing error: {}", err.to_string())).unwrap().into_bytes();
          (status_code, body)
        }
      }
    },
    Err(err) => {
      ic_cdk::println!("err in parsing utf8");
      let status_code = 400;
      let body = serde_json::to_string(&format!("Urf8 parsing error: {}", err.to_string())).unwrap().into_bytes();
      (status_code, body)
    }
  };

  HttpResponse {
    status_code,
    headers,
    body,
  }

}