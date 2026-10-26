use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=www");
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    if release && !Path::new("www/index.html").is_file() {
        panic!(
            "release builds embed www/, but www/index.html is missing; run frontend/build-shell.sh first"
        );
    }
}
