use reqwest::{Client, StatusCode};

pub(crate) struct Executor {
    endpoint: String,
}

impl Executor {
    pub(crate) fn new(node_address: &str, api_path: &str) -> Self {
        Self {
            endpoint: format!("http://{}{}", node_address, api_path),
        }
    }

    pub(crate) async fn execute<'a>(
        &self,
        json: &str,
    ) -> Result<(StatusCode, String), reqwest::Error> {
        let client = reqwest::Client::new(); // TODO: Create client in constructor

        Executor::json_resp(client, &self.endpoint, json.to_string()).await
    }

    async fn json_resp(
        client: Client,
        endpoint: &str,
        query: String,
    ) -> Result<(StatusCode, String), reqwest::Error> {
        let res = client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .body(query)
            .send()
            .await?;

        let status_code = res.status();
        let body = res.text().await?;
        Ok((status_code, body))
    }
}
