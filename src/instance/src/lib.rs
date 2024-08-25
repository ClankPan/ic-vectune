mod auth;
mod build_api;
mod consts;
pub mod ic_types;
mod service_api;
pub mod simd_point;
mod sysytem_api;
mod thread_locals;
pub mod types;

pub mod point;

pub mod graph;

use candid::Principal;
use build_api::StatusForFrontend;

// use simd_point::Point as SIMDPoint;

/* types */
use thread_locals::*;
use types::*;
use consts::*;

/* Set custom random function */
use getrandom::register_custom_getrandom;
use rand::RngCore;
// See here : https://forum.dfinity.org/t/issue-about-generate-random-string-panicked-at-could-not-initialize-thread-rng-getrandom-this-target-is-not-supported/15198/8?u=kinicdevcontributor
fn custom_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    RNG.with(|rng| rng.borrow_mut().as_mut().unwrap().fill_bytes(buf));
    Ok(())
}
register_custom_getrandom!(custom_getrandom);

/*
todo note

挿入と削除のapiを用意して、定期実行オプションと、手動のオプションをつける。


*/

/* !!Should be end of this file!! */
// Enable Candid export
// cargo build --release --target wasm32-unknown-unknown --package instance
// candid-extractor target/wasm32-unknown-unknown/release/instance.wasm > src/instance/instance.did
ic_cdk::export_candid!();
