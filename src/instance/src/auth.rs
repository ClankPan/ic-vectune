use candid::Principal;

use crate::thread_locals::*;

pub fn is_owner() -> Result<(), String> {
    let caller = ic_cdk::api::caller();

    OWNERS.with(|owners| {
        let owners = owners.borrow();
        if let Some(_) = owners.get(&caller.to_text()) {
            Ok(())
        } else {
            Err(String::from("Invalid id"))
        }
    })
}

pub fn add_owner(new_owner_pid: Principal, level: u8) {
    OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        owners.insert(new_owner_pid.to_text(), level);
    });
}
