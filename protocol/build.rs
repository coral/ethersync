use prost::Message;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "proto/ethersync/v1/ethersync.proto";
    println!("cargo:rerun-if-changed={path}");
    let descriptors = protox::compile([path], ["proto"])?;
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    std::fs::write(out.join("ethersync.bin"), descriptors.encode_to_vec())?;
    prost_build::Config::new().compile_fds(descriptors)?;
    Ok(())
}
