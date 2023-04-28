use std::borrow::Cow;
use std::{collections::HashMap, time::Duration};

use anyhow::Context;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct Client {
    client: reqwest::blocking::Client,
    token: String,
}

pub struct Req {
    pub method: Method,
    pub url_suffix: Cow<'static, str>,
    pub headers: HashMap<Cow<'static, str>, Cow<'static, str>>,
    pub body: Option<Cow<'static, str>>,
}

impl Req {
    pub fn new(method: Method, url_suffix: impl Into<Cow<'static, str>>) -> Self {
        Self {
            method,
            url_suffix: url_suffix.into(),
            headers: HashMap::new(),
            body: None,
        }
    }

    pub fn header(
        mut self,
        key: impl Into<Cow<'static, str>>,
        value: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    pub fn json<T: Serialize>(mut self, value: &T) -> Self {
        self.body = Some(serde_json::to_string(value).unwrap().into());
        self
    }
}

impl Client {
    pub fn new(token: &str) -> Client {
        let client = reqwest::blocking::ClientBuilder::new()
            .timeout(Duration::from_secs(60 * 4))
            .build()
            .unwrap();
        Client {
            token: token.to_string(),
            client,
        }
    }

    pub fn request<E: Endpoint>(&self, endpoint: &E) -> anyhow::Result<E::Response> {
        let req = endpoint.req();
        let url = format!(
            "https://api.openai.com{}{}",
            if req.url_suffix.starts_with('/') {
                ""
            } else {
                "/"
            },
            req.url_suffix
        );

        let mut request_builder = self.client.request(req.method, &url);

        for (key, value) in req.headers {
            request_builder = request_builder.header(key.to_string(), value.to_string());
        }

        request_builder = request_builder.bearer_auth(&self.token);

        if let Some(body) = req.body {
            request_builder = request_builder.body(body.into_owned());
        }

        let response = request_builder
            .send()
            .context("Failed to send request to API")?;

        let status = response.status();
        let body = response
            .json::<Value>()
            .with_context(|| anyhow::anyhow!("Status: {status}. Failed to parse response body."))?;
        let body_pretty = serde_json::to_string_pretty(&body).unwrap();

        if !status.is_success() {
            return Err(anyhow::anyhow!("Status: {}. Body: {}", status, body_pretty));
        }

        serde_json::from_value::<E::Response>(body).map_err(|e| {
            anyhow::anyhow!("Error while parsing response: {} Body: {}", e, body_pretty)
        })
    }
}

pub trait Endpoint {
    /// The return type of the endpoint.
    type Response: for<'de> Deserialize<'de>;

    /// Encodes the struct into an HTTP request.
    fn req(&self) -> Req;
}
