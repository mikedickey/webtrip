# ApiClient

The `ApiClient` is the main entry point for all API operations. It manages configuration, authentication, and HTTP connection pooling.

## Construction

### JavaScript

```javascript
// Default client
const client = new ApiClient();

// With custom base URL
const client = ApiClient.withBaseUrl('https://test.jacktrip.com/api');
```

### Rust

```rust
// Default client
let client = ApiClient::new();

// With custom base URL
let client = ApiClient::with_base_url("https://test.jacktrip.com/api".to_string());
```

## Configuration Methods

### Base URL

| Method | JavaScript | Rust |
|--------|------------|------|
| Set | `setBaseUrl(url)` | `set_base_url(url)` |
| Get | `getBaseUrl()` | `get_base_url()` |

Default: `https://www.jacktrip.com/api`

### Authentication

| Method | JavaScript | Rust |
|--------|------------|------|
| Set token | `setBearerToken(token)` | `set_bearer_token(token)` |
| Clear token | `clearBearerToken()` | `clear_bearer_token()` |
| Check if set | `hasBearerToken()` | `has_bearer_token()` |

### Timeout

| Method | JavaScript | Rust |
|--------|------------|------|
| Set | `setTimeoutMs(ms)` | `set_timeout_ms(ms)` |
| Get | `getTimeoutMs()` | `get_timeout_ms()` |

Default: `10000` (10 seconds)

### User Agent

| Method | JavaScript | Rust |
|--------|------------|------|
| Set | `setUserAgent(agent)` | `set_user_agent(agent)` |

Default: `jacktrip-web/1.0`

### Custom Headers

| Method | JavaScript | Rust |
|--------|------------|------|
| Add | `addHeader(key, value)` | `add_header(key, value)` |
| Remove | `removeHeader(key)` | `remove_header(key)` |
| Clear all | `clearHeaders()` | `clear_headers()` |

## API Accessors

Access different API modules through these methods:

| Method | Returns | Description |
|--------|---------|-------------|
| `system()` | `SystemApi` | Health checks, regions, analytics |
| `users()` | `UsersApi` | User profiles and account management |
| `studios()` | `StudiosApi` | Virtual studio management |
| `devices()` | `DevicesApi` | JackTrip hardware devices |
| `events()` | `EventsApi` | Upcoming broadcasts |
| `streams()` | `StreamsApi` | Live streams and channels |
| `recordings()` | `RecordingsApi` | Recorded content |
| `billing()` | `BillingApi` | Subscriptions and payments |

## Example

```javascript
const client = new ApiClient();

// Configure
client.setBaseUrl('https://test.jacktrip.com/api');
client.setBearerToken('your-token');
client.setTimeoutMs(30000);
client.addHeader('X-Request-Id', 'abc123');

// Use API modules
const system = client.system();
const users = client.users();
const studios = client.studios();

// Make calls
const ping = await system.ping();
const user = await users.getCurrentUser();
const studioList = await studios.listStudios();
```

## Connection Reuse

The `ApiClient` internally uses connection pooling. Creating multiple API accessor objects (e.g., calling `client.system()` multiple times) does not create new HTTP connections. The underlying connection pool is shared.

```javascript
// These all share the same HTTP connection pool
const system1 = client.system();
const system2 = client.system();
const users = client.users();

// Efficient: connections are reused
await system1.ping();
await users.getCurrentUser();
await system2.listRegions();
```

