//! Sync da memória (wiki) PC ↔ notebook (F5.4). Reexporta as funções de `sleep`, onde mora
//! a infraestrutura de git/remoto.
#![allow(unused_imports)]
pub use crate::sleep::{
    MemoryCollectReport, MemorySyncReport, collect_memory, collect_wiki, merge_memory, pull_memory,
    pull_wiki, push_memory, push_wiki, sync_memory, sync_mirror, sync_wiki, wiki_repo,
};
