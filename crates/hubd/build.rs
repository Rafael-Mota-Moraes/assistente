//! Grava o commit do build no binário: `/api/health` o compara com o da UI servida
//! (F8.3, F8.7). Fora de um repositório git (tarball, CI sem histórico) vira "desconhecido".

use std::process::Command;

fn main() {
    // Num worktree o .git é um arquivo e estes caminhos não existem: o cargo então roda o
    // build de novo sempre, o que é justamente o que o atualizador (F8.5) precisa.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
    println!("cargo:rustc-env=HUBD_COMMIT={}", head());
}

fn head() -> String {
    let Some(short) = git(&["rev-parse", "--short", "HEAD"]) else {
        return "desconhecido".to_owned();
    };
    let dirty = git(&["status", "--porcelain"]).is_some_and(|out| !out.is_empty());
    format!("{short}{}", if dirty { "-sujo" } else { "" })
}

fn git(args: &[&str]) -> Option<String> {
    // O repositório fica dois níveis acima de `crates/hubd`; o cargo define o cwd do build.rs.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
