use bytesize::MIB;

pub const WASM_PAGE_SIZE: u64 = 65536; // 64 KiB
pub const MISSING_CHUNKS_RESPONCE_SIZE: usize = 2 * MIB as usize;

pub const VAMANA_GRAPH_MEMORY_ID: u8 = 0;
pub const METADATA_MEMORY_ID: u8 = 1;
pub const DATA_MAP_MEMORY_ID: u8 = 2;
pub const OWNERS_MEMORY_ID: u8 = 3;
pub const IC_STATUS_MEMORY_ID: u8 = 4;
pub const BACKLINKS_MEMORY_ID: u8 = 5;
