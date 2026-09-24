fn main() {
    // Export symbols so the dynamically loaded ALGO extension can link against liblbug.
    println!("cargo:rustc-link-arg=-rdynamic");
}
