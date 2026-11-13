fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_client(true)
        .compile_protos(&["proto/sunny/relay/v1/relay.proto"], &["proto"])?;
    Ok(())
}
