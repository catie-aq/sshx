fn main() {
    // Allow overriding the default server URL at build time.
    // Usage: SSHX_BUILD_URL=https://my.server.com cargo build -p sshx
    let default_url = std::env::var("SSHX_BUILD_URL")
        .unwrap_or_else(|_| "https://sshx.io".to_string());
    println!("cargo:rustc-env=SSHX_DEFAULT_SERVER_URL={default_url}");
    println!("cargo:rerun-if-env-changed=SSHX_BUILD_URL");
    println!("cargo:rerun-if-changed=build.rs");

    // Platform label, matching the keys of the server's release manifest.
    let target = std::env::var("TARGET").unwrap();
    println!("cargo:rustc-env=SSHX_PLATFORM={}", platform_label(&target));
}

/// Keep in sync with `platform_label` in scripts/publish-release.sh.
fn platform_label(target: &str) -> String {
    let arch = match target.split('-').next().unwrap_or("unknown") {
        "i586" | "i686" => "x86",
        "arm" => "armv6",
        a if a.starts_with("armv7") => "armv7",
        a => a,
    };
    let os = if target.contains("linux") {
        "linux"
    } else if target.contains("windows") {
        "windows"
    } else if target.contains("apple") {
        "macos"
    } else if target.contains("freebsd") {
        "freebsd"
    } else {
        "unknown"
    };
    format!("{os}-{arch}")
}
