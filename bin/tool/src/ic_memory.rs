use candid::{Decode, Encode};
use ic_stable_structures::{storable::Bound, BTreeMap as StableBTreeMap, Memory, Storable};
use std::{borrow::Cow, cell::RefCell, collections::HashSet};

pub const WASM_PAGE_SIZE: u64 = 65536; // 64 KiB

pub struct ICMemory {
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

pub struct ICRBTree<K, V>
where
    K: Ord + Storable + Clone,
    V: Storable,
{
    ic_rbtree: StableBTreeMap<K, V, ICMemory>,
}

impl<K, V> ICRBTree<K, V>
where
    K: Ord + Storable + Clone,
    V: Storable,
{
    //   pub fn new() -> Self {
    //       let ic_rbtree = StableBTreeMap::new(ICMemory {
    //           mem: RefCell::new(vec![]),
    //       });
    //       Self { ic_rbtree }
    //   }

    //   pub fn insert(&mut self, key: K, value: V) -> Option<V> {
    //       self.ic_rbtree.insert(key, value)
    //   }

    //   pub fn get(&mut self, key: K) -> Option<V> {
    //       self.ic_rbtree.get(&key)
    //   }

    pub fn load_memory(memory: Vec<u8>) -> Self {
        let ic_rbtree = StableBTreeMap::<K, V, ICMemory>::init(ICMemory {
            mem: RefCell::new(memory),
        });
        Self { ic_rbtree }
    }

    pub fn get_items(&self) -> Vec<(K, V)> {
        self.ic_rbtree.iter().collect()
    }

    pub fn build_from_vec(items: Vec<(K, V)>) -> Self {
        let mut ic_rbtree = StableBTreeMap::new(ICMemory {
            mem: RefCell::new(vec![]),
        });
        for (k, v) in items.into_iter() {
            let _ = ic_rbtree.insert(k, v);
        }

        Self { ic_rbtree }
    }

    pub fn into_memory(self) -> Vec<u8> {
        self.ic_rbtree.into_memory().mem.into_inner()
    }
}

#[derive(candid::CandidType, candid::Deserialize, Clone, Debug)]
// #[derive(serde::Serialize, serde::Deserialize)]
pub struct Backlinks(pub HashSet<u32>);

impl Storable for Backlinks {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Unbounded;
}
