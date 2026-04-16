fn main() -> Result<(), Box<dyn std::error::Error>> {
    prost_build::Config::new()
        .out_dir("src/proto")
        .compile_protos(&["proto/tunnel.proto"], &["proto/"])?;
    Ok(())
}
