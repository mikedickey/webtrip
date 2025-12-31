# JackTrip Virtual Studio API Client

A clean, ergonomic API client for the JackTrip Virtual Studio platform. This library works seamlessly in both **Rust** and **JavaScript/TypeScript** via WebAssembly.

## Features

- **Dual Language Support**: Use the same API from Rust or JavaScript/TypeScript
- **Fully Typed**: Strong typing in both Rust and TypeScript
- **Async/Await**: All API calls are asynchronous
- **Connection Reuse**: HTTP connections are pooled and reused
- **Modular Design**: APIs are organized into logical modules

## API Modules

| Module | Description |
|--------|-------------|
| [System](./system.md) | Health checks, regions, analytics |
| [Users](./users.md) | User profiles, notifications, referrals |
| [Studios](./studios.md) | Virtual studio management |
| [Devices](./devices.md) | JackTrip hardware devices |
| [Events](./events.md) | Upcoming broadcasts and events |
| [Streams](./streams.md) | Live streams and channels |
| [Recordings](./recordings.md) | Recorded content management |
| [Billing](./billing.md) | Subscriptions and payments |

## Quick Start

### JavaScript/TypeScript

```javascript
import init, { ApiClient } from 'jacktrip-web';

// Initialize the WASM module
await init();

// Create client
const client = new ApiClient();

// Optional: Set authentication token
client.setBearerToken('your-jwt-token');

// Make API calls
const regions = await client.system().listRegions();
const user = await client.users().getCurrentUser();
```

### Rust

```rust
use jacktrip_web::api::ApiClient;

// Create client
let mut client = ApiClient::new();

// Optional: Set authentication token
client.set_bearer_token("your-jwt-token".to_string());

// Make API calls
let regions = client.system().list_regions().await?;
let user = client.users().get_current_user().await?;
```

## Documentation

- [Getting Started](./getting-started.md) - Installation and setup
- [API Client](./api-client.md) - Client configuration
- [Error Handling](./error-handling.md) - Working with errors

## Authentication

Most endpoints require authentication via JWT bearer token. You can either:

1. **Set the bearer token explicitly:**
   ```javascript
   client.setBearerToken('your-jwt-token');
   ```

2. **Use cookies**: If you're already authenticated in a browser session, cookies will be sent automatically (when not setting a bearer token).

## Base URL

The default base URL is `https://www.jacktrip.com/api`. You can customize it:

```javascript
// At construction
const client = ApiClient.withBaseUrl('https://custom.api.com/api');

// Or after
client.setBaseUrl('https://custom.api.com/api');
```
