# Getting Started

## Installation

### JavaScript/TypeScript (npm)

```bash
npm install jacktrip-web
```

### Rust (Cargo)

```toml
[dependencies]
jacktrip-web = "0.1"
```

## Basic Usage

### JavaScript/TypeScript

```javascript
import init, { ApiClient } from 'jacktrip-web';

async function main() {
  // Initialize the WASM module (required once)
  await init();

  // Create an API client
  const client = new ApiClient();

  // Check API health (unauthenticated)
  const ping = await client.system().ping();
  console.log('API Version:', ping.version);

  // List available regions (unauthenticated)
  const regions = await client.system().listRegions();
  console.log(`Found ${regions.length} regions`);

  // For authenticated endpoints, set the bearer token
  client.setBearerToken('your-jwt-token');

  // Get current user
  const user = await client.users().getCurrentUser();
  console.log('Hello,', user.displayName);

  // List user's studios
  const studios = await client.studios().listStudios();
  console.log(`You have ${studios.length} studios`);
}

main().catch(console.error);
```

### Rust

```rust
use jacktrip_web::api::{ApiClient, ApiError};

#[tokio::main]
async fn main() -> Result<(), ApiError> {
    // Create an API client
    let mut client = ApiClient::new();

    // Check API health (unauthenticated)
    let ping = client.system().ping().await?;
    println!("API Version: {:?}", ping.version);

    // List available regions (unauthenticated)
    let regions = client.system().list_regions().await?;
    println!("Found {} regions", regions.len());

    // For authenticated endpoints, set the bearer token
    client.set_bearer_token("your-jwt-token".to_string());

    // Get current user
    let user = client.users().get_current_user().await?;
    println!("Hello, {:?}", user.display_name);

    // List user's studios
    let studios = client.studios().list_studios().await?;
    println!("You have {} studios", studios.len());

    Ok(())
}
```

## Configuration Options

### Base URL

```javascript
// Custom base URL at construction
const client = ApiClient.withBaseUrl('https://test.jacktrip.com/api');

// Or change it later
client.setBaseUrl('https://test.jacktrip.com/api');
```

### Timeout

```javascript
// Set timeout to 30 seconds (in milliseconds)
client.setTimeoutMs(30000);
```

### Custom Headers

```javascript
// Add a custom header
client.addHeader('X-Custom-Header', 'value');

// Remove a header
client.removeHeader('X-Custom-Header');

// Clear all custom headers
client.clearHeaders();
```

### User Agent

```javascript
client.setUserAgent('MyApp/1.0');
```

## Authentication

### Bearer Token

Most authenticated endpoints accept a JWT bearer token:

```javascript
client.setBearerToken('eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...');

// Check if token is set
if (client.hasBearerToken()) {
  console.log('Authenticated');
}

// Clear the token
client.clearBearerToken();
```

### Cookie-based Authentication

If you're using the API from a browser where the user is already logged in, cookies will be sent automatically. No explicit token setting is required.

## Error Handling

All API methods return Promises (JavaScript) or Results (Rust) that may fail:

### JavaScript

```javascript
try {
  const user = await client.users().getCurrentUser();
} catch (error) {
  console.error('API Error:', error);
}
```

### Rust

```rust
match client.users().get_current_user().await {
    Ok(user) => println!("User: {:?}", user),
    Err(ApiError::Http { status, message, .. }) => {
        println!("HTTP {} Error: {}", status, message);
    }
    Err(e) => println!("Error: {}", e),
}
```

See [Error Handling](./error-handling.md) for more details.

