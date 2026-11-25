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

# async fn run() -> Result<(), ApiError> {
let client = ApiClient::new();
let ping = client.system().ping().await?;

client.set_bearer_token("your-jwt-token".to_string());
let studios = client.studios().list_studios().await?;
println!("{} studios", studios.len());
# Ok(()) }
```

## Features

- `wasm` — `#[wasm_bindgen]` exports and generated TypeScript types. `webtrip-web`
  enables it for the browser package.

## See also

- [`webtrip`](https://docs.rs/webtrip): the one-dependency facade over all WebTrip crates
- [Getting started](https://webtrip.dev/docs/getting-started)
