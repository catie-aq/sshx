fn main() {
    // Allow overriding the default server URL at build time.
    // Usage: SSHX_BUILD_URL=https://my.server.com cargo build -p sshx
    let default_url = std::env::var("SSHX_BUILD_URL")
        .unwrap_or_else(|_| "https://sshx.io".to_string());
    println!("cargo:rustc-env=SSHX_DEFAULT_SERVER_URL={default_url}");
    println!("cargo:rerun-if-env-changed=SSHX_BUILD_URL");
}
