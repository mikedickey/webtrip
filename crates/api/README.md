# webtrip-api

A typed, async client for the [JackTrip Virtual Studio](https://www.jacktrip.com)
REST API, part of [WebTrip](https://webtrip.dev).

The same client works from Rust and, via the `wasm` feature, from JavaScript and
TypeScript, with TypeScript types generated for every model. It is portable: no
browser dependencies, and it builds natively and on `wasm32-unknown-unknown`.

- [`api`] — [`ApiClient`](api::ApiClient) and one sub-client per area: system,
  users, studios, devices, events, streams, recordings, billing, subscriptions, tracks
- [`models`] — request and response types that match the API's JSON shapes

## Example

```rust,no_run
use webtrip_api::api::{ApiClient, ApiError};

#[tokio::main]
async fn main() -> Result<(), ApiError> {
    // `ApiClient::new()` targets production; this is the test environment.
    let client = ApiClient::with_base_url("https://test.jacktrip.com/api".to_string());
    let ping = client.system().ping().await?;
    println!("API status: {:?}", ping.status);

    client.set_bearer_token("your-jwt-token".to_string());
    let studios = client.studios().list_studios().await?;
    println!("{} studios", studios.len());
    Ok(())
}
```

## Using the client

- **Environments.** [`ApiClient::new`](api::ApiClient::new) talks to production
  (`https://www.jacktrip.com/api`); use
  [`ApiClient::with_base_url`](api::ApiClient::with_base_url) with
  `https://test.jacktrip.com/api` for the test environment.
- **Authentication.** Most endpoints need a JWT from JackTrip's Auth0 tenant,
  set with [`set_bearer_token`](api::ApiClient::set_bearer_token). The client
  never obtains or refreshes tokens: on an HTTP `401`, fetch a new token and set
  it again. Every API object obtained from the client picks up the new token.
- **Runtime.** Natively the client uses `reqwest` with `rustls` and needs a
  [tokio](https://docs.rs/tokio) runtime. In the browser it uses `fetch`.
- **Sharing.** `ApiClient` is cheap to clone and, natively, `Send + Sync`;
  clones share one configuration.
- **Errors.** Every call returns
  [`Result<T, ApiError>`](api::ApiError); see the
  [`api` module docs](api#errors) for what each variant means.

## Features

- `wasm` — `#[wasm_bindgen]` exports and generated TypeScript types. `webtrip-web`
  enables it for the browser package.

## See also

- [`webtrip`](https://docs.rs/webtrip): the one-dependency facade over all WebTrip crates
- [Getting started](https://webtrip.dev/docs/getting-started)
