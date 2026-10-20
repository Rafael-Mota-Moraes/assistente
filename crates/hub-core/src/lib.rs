//! Domínio puro do Assistente, compartilhado pelo `hubd` (na VM) e pelo app Tauri (no host).
//!
//! Nada aqui abre socket nem processo: só lê config, descobre projetos e monta comandos.
//! Assim dá para testar tudo sem VM, tmux ou notebook.

pub mod config;
pub mod project;
pub mod project_settings;
pub mod session;
pub mod sleep;

pub use config::{Config, ConfigError, Profile, expand_home};
pub use project::{Project, discover_projects};
pub use project_settings::{ProjectSettings, SettingsFile, load_settings, settings_path};
pub use session::{
    AgentKind, ExecAccess, LaunchOptions, LaunchPlan, SessionSpec, claude_logged_in,
    exec_client_dir, profile_data_dir,
};
