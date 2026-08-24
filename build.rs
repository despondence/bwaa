fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = &[
        // v1beta (chat)
        "subprojects/googleapis/google/ai/generativelanguage/v1beta/generative_service.proto",
        "subprojects/googleapis/google/ai/generativelanguage/v1beta/content.proto",
        // v1alpha (live)
        "subprojects/googleapis/google/ai/generativelanguage/v1alpha/generative_service.proto",
        "subprojects/googleapis/google/ai/generativelanguage/v1alpha/content.proto",
        // deps
        "subprojects/googleapis/google/type/interval.proto",
        "subprojects/googleapis/google/type/latlng.proto",
        "subprojects/googleapis/google/type/date.proto",
        "subprojects/googleapis/google/rpc/status.proto",
    ];

    let includes = &["subprojects/googleapis"];

    tonic_prost_build::configure()
        .build_server(false)
        .compile_protos(protos, includes)?;

    Ok(())
}
