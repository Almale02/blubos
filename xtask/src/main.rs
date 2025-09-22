use std::{
    env, fs,
    path::Path,
    process::{Command, exit},
};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let kernel_path = format!("target/x86_64-blubos/release/kernel");
    let boot_kernel = "boot/kernel.elf";

    run_in_dir("cargo", &["build", "--release"], Path::new("kernel"));

    fs::create_dir_all("boot").unwrap();

    fs::copy(&kernel_path, boot_kernel).unwrap_or_else(|e| {
        eprintln!("❌ Failed to copy kernel: {e}");
        exit(1);
    });
    println!("✅ Copied {kernel_path} → {boot_kernel}");

    // Run makeimg.sh
    run("bash", &["makeimg.sh"]);

    // If no extra args → run qemu by default
    if args.get(0).map(|s| s.as_str()) == Some("run") {
        run("bash", &["./img/run_qemu.sh"]);
    }
}
fn run(cmd: &str, args: &[&str]) {
    println!("▶️  {cmd} {}", args.join(" "));
    let status = Command::new(cmd).args(args).status().unwrap();
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

fn run_in_dir(cmd: &str, args: &[&str], dir: &Path) {
    println!("▶️  cd {} && {cmd} {}", dir.display(), args.join(" "));
    let status = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}
