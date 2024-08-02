use crate::{auth::*, thread_locals::*, types::*};
use candid::Principal;
use ic_cdk::{init, post_upgrade};
use rand::{rngs::StdRng, SeedableRng};
use std::time::Duration;

#[init] //  dfx deploy storage --mode=reinstall
fn init(arg: (Principal, String, String)) {
    ic_cdk::println!("initializing rand seed ...");
    set_rng();
    let installer = ic_cdk::api::caller();
    add_owner(installer, 0);
    // let arg = ic_cdk::api::call::arg_data::<(Principal, String, String)>(); // owner_pid, name, version
    let owner = arg.0;
    let name = arg.1;
    let version = arg.2;
    ic_cdk::println!("owner is {}", owner.to_text());
    add_owner(owner, 0);
    let initial_status = IcStatus {
        controllers: vec![],
        compute_allocation: 0,
        memory_allocation: 0,
        freezing_threshold: 0,
        module_hash: None,
        memory_size: 0,
        cycles: 0,
        idle_cycles_burned_per_day: 0,
    };
    IcStatus::update(&initial_status);

    METADATA.with(|metadata| {
        let mut metadata = metadata.borrow_mut();
        let _ = metadata.set(Metadata::Initial(InitialMetadata { name, version }));
    });
}

#[post_upgrade] //  dfx deploy storage --mode=reinstall
fn post_upgrade(arg: (Principal, String, String)) {
    ic_cdk::println!("initializing rand seed ...");
    set_rng();
    let name = arg.1;
    let version = arg.2;

    Metadata::change_name(name);
    Metadata::change_version(version)
}

fn set_rng() {
    ic_cdk_timers::set_timer(Duration::ZERO, || {
        ic_cdk::spawn(async {
            let (seed,): ([u8; 32],) =
                ic_cdk::call(Principal::management_canister(), "raw_rand", ())
                    .await
                    .unwrap();
            RNG.with(|rng| *rng.borrow_mut() = Some(StdRng::from_seed(seed)));
        })
    });
}
