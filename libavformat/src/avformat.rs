#[cfg(feature = "generate_avformat_rs")]
include!(concat!(env!("OUT_DIR"), "/avformat.rs"));

#[cfg(not(feature = "generate_avformat_rs"))]
include!("avformat-backup-56.rs");
