use prost::Message;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Deterministic source identity shared by native, bindings and WASM builds.
    // Not a security hash: this detects accidentally mixed local SDK revisions.
    let mut sources: Vec<_> = std::fs::read_dir("src")?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    sources.retain(|p| p.extension().is_some_and(|ext| ext == "rs"));
    sources.extend(["Cargo.toml".into(), "proto/tidkod/v1/tidkod.proto".into()]);
    sources.sort();
    let mut hash = 0xcbf29ce484222325u64;
    for source in sources {
        println!("cargo:rerun-if-changed={}", source.display());
        for byte in std::fs::read(source)? {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    println!("cargo:rustc-env=TIDKOD_CORE_ID={hash:016x}");
    let path = "proto/tidkod/v1/tidkod.proto";
    println!("cargo:rerun-if-changed={path}");
    let descriptors = protox::compile([path], ["proto"])?;
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    std::fs::write(out.join("tidkod.bin"), descriptors.encode_to_vec())?;
    prost_build::Config::new().compile_fds(descriptors)?;
    Ok(())
}
