use std::time::Duration;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Request, Streaming};

use crate::authorisation::Authorisation;
use crate::googleapis::google::ai::generativelanguage::v1beta::generative_service_client::GenerativeServiceClient;
use crate::googleapis::google::ai::generativelanguage::v1beta::{
    CountTokensRequest, CountTokensResponse, GenerateContentRequest, GenerateContentResponse,
};

const GEMINI_API_ENDPOINT: &str = "https://generativelanguage.googleapis.com";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// High-level async gRPC client for Gemini `v1beta` API.
#[derive(Clone)]
pub struct GeminiClient {
    inner: GenerativeServiceClient<InterceptedService<Channel, Authorisation>>,
}

impl GeminiClient {
    /// Connects to the Google Generative Language gRPC endpoint with TLS and API key authorisation.
    pub async fn connect(api_key: impl AsRef<str>) -> anyhow::Result<Self> {
        let auth: Authorisation = api_key.as_ref().parse()?;

        let tls_config = ClientTlsConfig::new().with_enabled_roots();

        let endpoint = Endpoint::from_static(GEMINI_API_ENDPOINT)
            .tls_config(tls_config)?
            .timeout(DEFAULT_TIMEOUT)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .http2_keep_alive_interval(Duration::from_secs(30));

        let channel = endpoint.connect().await?;
        let client = GenerativeServiceClient::with_interceptor(channel, auth);

        Ok(Self { inner: client })
    }

    /// Unary content generation request (used by Agent turn loops and tool resolution).
    pub async fn generate_content(
        &mut self,
        request: GenerateContentRequest,
    ) -> anyhow::Result<GenerateContentResponse> {
        let response = self
            .inner
            .generate_content(Request::new(request))
            .await?
            .into_inner();

        Ok(response)
    }

    /// Server-streaming content generation request.
    pub async fn stream_generate_content(
        &mut self,
        request: GenerateContentRequest,
    ) -> anyhow::Result<Streaming<GenerateContentResponse>> {
        let stream = self
            .inner
            .stream_generate_content(Request::new(request))
            .await?
            .into_inner();

        Ok(stream)
    }

    /// Helper to count tokens for a given prompt/context payload.
    pub async fn count_tokens(
        &mut self,
        request: CountTokensRequest,
    ) -> anyhow::Result<CountTokensResponse> {
        let response = self
            .inner
            .count_tokens(Request::new(request))
            .await?
            .into_inner();

        Ok(response)
    }
}
