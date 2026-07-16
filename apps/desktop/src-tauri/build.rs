fn main() {
    // Embed short git SHA for the in-app updater (CI sets GITHUB_SHA).
    let sha = std::env::var("GITHUB_SHA")
        .or_else(|_| std::env::var("LANPLAY_GIT_SHA"))
        .unwrap_or_else(|_| "dev".into());
    let short: String = sha.chars().take(12).collect();
    println!("cargo:rustc-env=LANPLAY_GIT_SHA={short}");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=LANPLAY_GIT_SHA");
    tauri_build::build()
}
