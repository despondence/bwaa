use crate::authorisation::Authorisation;
use crate::googleapis::google::ai::generativelanguage::{v1alpha, v1beta};

use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

const ENDPOINT: &str = "https://generativelanguage.googleapis.com";

/// Client for standard Gemini v1beta gRPC endpoints.
pub struct Gemini {
    client: v1beta::generative_service_client::GenerativeServiceClient<
        InterceptedService<Channel, Authorisation>,
    >,
}

impl Gemini {
    pub async fn connect(api_key: String) -> anyhow::Result<Self> {
        let tls_config = ClientTlsConfig::new().with_enabled_roots();

        let channel = Endpoint::from_static(ENDPOINT)
            .tls_config(tls_config)?
            .connect()
            .await?;

        let client = v1beta::generative_service_client::GenerativeServiceClient::with_interceptor(
            channel,
            api_key.parse()?,
        );

        Ok(Self { client })
    }

    /// Unary content generation.
    pub async fn generate_content(
        &mut self,
        request: v1beta::GenerateContentRequest,
    ) -> anyhow::Result<v1beta::GenerateContentResponse> {
        let response = self.client.generate_content(request).await?.into_inner();

        Ok(response)
    }

    /// Server-streaming content generation.
    pub async fn stream_generate_content(
        &mut self,
        request: v1beta::GenerateContentRequest,
    ) -> anyhow::Result<tonic::Streaming<v1beta::GenerateContentResponse>> {
        let stream = self
            .client
            .stream_generate_content(request)
            .await?
            .into_inner();

        Ok(stream)
    }
}

/// Client for Gemini v1alpha gRPC endpoints (includes Live API / Bidi streaming).
pub struct GeminiAlpha {
    client: v1alpha::generative_service_client::GenerativeServiceClient<
        InterceptedService<Channel, Authorisation>,
    >,
}

impl GeminiAlpha {
    pub async fn connect(api_key: String) -> anyhow::Result<Self> {
        let tls_config = ClientTlsConfig::new().with_enabled_roots();

        let channel = Endpoint::from_static(ENDPOINT)
            .tls_config(tls_config)?
            .connect()
            .await?;

        let client = v1alpha::generative_service_client::GenerativeServiceClient::with_interceptor(
            channel,
            api_key.parse()?,
        );

        Ok(Self { client })
    }

    /// Unary content generation (v1alpha).
    pub async fn generate_content(
        &mut self,
        request: v1alpha::GenerateContentRequest,
    ) -> anyhow::Result<v1alpha::GenerateContentResponse> {
        let response = self.client.generate_content(request).await?.into_inner();

        Ok(response)
    }

    /// Server-streaming content generation (v1alpha).
    pub async fn stream_generate_content(
        &mut self,
        request: v1alpha::GenerateContentRequest,
    ) -> anyhow::Result<tonic::Streaming<v1alpha::GenerateContentResponse>> {
        let stream = self
            .client
            .stream_generate_content(request)
            .await?
            .into_inner();

        Ok(stream)
    }

    /// Full-duplex bidirectional streaming for Gemini Live API.
    pub async fn bidi(
        &mut self,
        stream: UnboundedReceiver<v1alpha::BidiGenerateContentClientMessage>,
    ) -> anyhow::Result<tonic::Streaming<v1alpha::BidiGenerateContentServerMessage>> {
        let stream = self
            .client
            .bidi_generate_content(UnboundedReceiverStream::new(stream))
            .await?
            .into_inner();

        Ok(stream)
    }
}
