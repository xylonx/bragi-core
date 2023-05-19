use std::time::Duration;

use anyhow::{anyhow, Result};
use futures::{
    channel::{mpsc, oneshot},
    SinkExt, StreamExt,
};
use log::error;
use reqwest::{Client, Request, Response};
use tower::{Service, ServiceExt};

#[derive(Debug)]
pub struct LimitedRequestClient {
    request_tx: mpsc::Sender<(Request, oneshot::Sender<Result<Response>>)>,
}

impl LimitedRequestClient {
    /// [buffer] -> [concurrency req pool] - :{rate limit}: -> client.call()
    pub fn new(
        client: Client,
        channel_buffer_size: usize,
        request_buffer_size: usize,
        max_concurrency_number: usize,
        rate_limit_number: u64,
        rate_limit_duration: Duration,
    ) -> Self {
        let (tx, rx) =
            mpsc::channel::<(Request, oneshot::Sender<Result<Response>>)>(channel_buffer_size); // update the magic number

        tokio::spawn(async move {
            let service = tower::ServiceBuilder::new()
                .buffer(request_buffer_size)
                .concurrency_limit(max_concurrency_number)
                .rate_limit(rate_limit_number, rate_limit_duration)
                .service(client.clone());
            rx.for_each_concurrent(max_concurrency_number, move |(req, resp_tx)| {
                let mut inner_service = service.clone();
                async move {
                    let resp = match inner_service.ready().await {
                        Ok(srv) => match srv.call(req).await {
                            Ok(r) => Ok(r),
                            Err(e) => Err(anyhow!(
                                "LimitedRequestClient: service call request failed: {}",
                                e
                            )),
                        },
                        Err(e) => Err(anyhow!("LimitedRequestClient: service ready failed: {}", e)),
                    };
                    match resp_tx.send(resp) {
                        Ok(_) => (),
                        Err(_) => error!(
                            "LimitedRequestClient: send resp to resp_tx failed: channel closed"
                        ),
                    }
                }
            })
            .await // prevent for_each_concurrent return to keep it in-flight
        });
        Self { request_tx: tx }
    }

    pub async fn call(&self, req: Request) -> Result<Response> {
        let (tx, rx) = oneshot::channel::<Result<Response>>();
        self.request_tx.clone().send((req, tx)).await?;
        rx.await?
    }
}

// #[cfg(test)]
// mod tests {
//     use log::info;
//     use reqwest::{Method, Url};

//     use super::*;

//     #[tokio::test]
//     async fn test_concurrency_request() {
//         env_logger::init();

//         let client =
//             LimitedRequestClient::new(Client::default(), 10, 100, 5, Duration::from_secs(1));

//         futures::future::join_all({ 0..100 }.map(|_| {
//             let c = &client;
//             async move {
//                 let req =
//                     reqwest::Request::new(Method::GET, Url::parse("https://google.com").unwrap());
//                 let resp = c.request(req).await;
//                 info!("resp: {:?}", resp);
//             }
//         }))
//         .await;
//     }
// }
