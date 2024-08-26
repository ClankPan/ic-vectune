use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BTreeMap as StableBTreeMap, Cell as StableCell, Vec as SVec, DefaultMemoryImpl};

use rand::rngs::StdRng;
use std::cell::RefCell;
use std::rc::Rc;

use crate::consts::*;
use crate::types::*;

pub type VMemory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
  // The memory manager is used for simulating multiple memories. Given a `MemoryId` it can
  // return a memory that can be used by stable structures.
  pub static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
    RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    pub static RNG: RefCell<Option<StdRng>> = RefCell::new(None);
}

thread_local! {
  pub static METADATA: RefCell<StableCell::<Metadata, VMemory>>= RefCell::new(
      StableCell::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(METADATA_MEMORY_ID))),
          Metadata::None
      ).unwrap()
  );

  pub static SOURCE_DATA: RefCell<StableBTreeMap<u32, String, VMemory>> = RefCell::new(
    StableBTreeMap::init(
      MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(DATA_MAP_MEMORY_ID))),
    )
  );

  pub static OWNERS: Rc<RefCell<StableBTreeMap<String, u8, VMemory>>> = { // u8 is owner's auth level
    Rc::new(
      RefCell::new(
        StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(OWNERS_MEMORY_ID))),
        )
      )
    )
  };

  pub static IC_STATUS: RefCell<StableBTreeMap<u8, IcStatus, VMemory>> = RefCell::new(
    StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(IC_STATUS_MEMORY_ID))))
  );

  pub static BACKLINKS_MAP: RefCell<StableBTreeMap<u32, Backlinks, VMemory>> = RefCell::new(
    StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(BACKLINKS_MEMORY_ID))))
  );

  pub static FREE_ID_LIST: RefCell<SVec<u32, VMemory>> = RefCell::new(
    SVec::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(FREE_ID_LIST_MEMORY_ID)))).unwrap()
  );

  pub static BATCH_POOL: RefCell<SVec<OptType, VMemory>> = RefCell::new(
    SVec::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(BATCH_POOL_MEMORY_ID)))).unwrap()
  );

  pub static CEMETERY: RefCell<SVec<u32, VMemory>> = RefCell::new(
    SVec::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(CEMETERY_MEMORY_ID)))).unwrap()
  );
}
