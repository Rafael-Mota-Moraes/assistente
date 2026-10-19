//! Domínio puro do Assistente, compartilhado pelo `hubd` (na VM) e pelo app Tauri (no host).
//!
//! Nada aqui abre socket nem processo: só lê config, descobre projetos e monta comandos.
//! Assim dá para testar tudo sem VM, tmux ou notebook.

pub mod config;
pub mod project;
pub mod session;
pub mod sleep;

pub use config::{Config, ConfigError, Profile, expand_home};
pub use project::{Project, discover_projects};
pub use session::{AgentKind, LaunchPlan, SessionSpec, profile_data_dir};
