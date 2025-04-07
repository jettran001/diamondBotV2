fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tạm thời bỏ qua việc biên dịch protobuf do chưa cài đặt protoc
    // tonic_build::compile_protos("proto/snipebot.proto")?;
    Ok(())
}
