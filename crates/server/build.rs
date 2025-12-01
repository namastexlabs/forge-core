fn main() {
    dotenv::dotenv().ok();

    if let Ok(api_key) = std::env::var("POSTHOG_API_KEY") {
        println!("cargo:rustc-env=POSTHOG_API_KEY={}", api_key);
    }
    if let Ok(api_endpoint) = std::env::var("POSTHOG_API_ENDPOINT") {
        println!("cargo:rustc-env=POSTHOG_API_ENDPOINT={}", api_endpoint);
    }
    if let Ok(api_key) = std::env::var("GITHUB_APP_ID") {
        println!("cargo:rustc-env=GITHUB_APP_ID={}", api_key);
    }
    if let Ok(api_endpoint) = std::env::var("GITHUB_APP_CLIENT_ID") {
        println!("cargo:rustc-env=GITHUB_APP_CLIENT_ID={}", api_endpoint);
    }
}
