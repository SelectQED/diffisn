use std::process::{exit, Command};

fn main() {
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to get current exe path: {}", e);
            exit(1);
        }
    };

    let mut sibling = current_exe.clone();
    sibling.set_file_name("diffisn");

    let status = Command::new(&sibling)
        .args(std::env::args().skip(1))
        .env("DIFFISN_VERBOSE", "1")
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Failed to run {}: {}", sibling.display(), e);
            exit(1);
        });

    exit(status.code().unwrap_or(1));
}
