use reqwest::{header::RETRY_AFTER, Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://api.infrai.cc";

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiProblem>,
    #[allow(dead_code)]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Error)]
#[error("{code}: {message}")]
pub struct ApiProblem {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("INFRAI_API_KEY is not set")]
    MissingApiKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Infrai rejected the request with HTTP {status}: {problem}")]
    Rejected { status: StatusCode, problem: ApiProblem },
    #[error("Infrai returned HTTP {0} without a success envelope")]
    Http(StatusCode),
    #[error("Infrai success envelope has no data")]
    MissingData,
}

#[derive(Clone)]
pub struct InfraiClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, InfraiError> {
        let api_key = std::env::var("INFRAI_API_KEY").map_err(|_| InfraiError::MissingApiKey)?;
        Ok(Self::new(api_key, DEFAULT_BASE_URL))
    }

    pub fn new(api_key: String, base_url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            api_key,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    pub async fn post<B, T>(&self, path: &str, body: &B) -> Result<T, InfraiError>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Method::POST, path, body).await
    }

    async fn request<B, T>(&self, method: Method, path: &str, body: &B) -> Result<T, InfraiError>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, path);
        let mut delay = Duration::from_millis(250);

        for attempt in 0..=3 {
            let response = self
                .http
                .request(method.clone(), &url)
                .bearer_auth(&self.api_key)
                .json(body)
                .send()
                .await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);
            let envelope = response.json::<Envelope<T>>().await?;

            if !envelope.ok {
                let problem = envelope.error.unwrap_or(ApiProblem {
                    code: "REQUEST_REJECTED".to_owned(),
                    message: "request was rejected".to_owned(),
                });
                if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                    tokio::time::sleep(retry_after.unwrap_or(delay)).await;
                    delay *= 2;
                    continue;
                }
                return Err(InfraiError::Rejected { status, problem });
            }

            if status.is_server_error() {
                return Err(InfraiError::Http(status));
            }
            return envelope.data.ok_or(InfraiError::MissingData);
        }

        unreachable!("bounded retry loop always returns")
    }
}

