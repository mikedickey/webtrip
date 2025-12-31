# System API

Health checks, region information, and analytics. Most endpoints in this module are unauthenticated.

## Access

```javascript
const systemApi = client.system();
```

```rust
let system_api = client.system();
```

## Methods

### ping

Check API health and get version information.

**Authentication:** None required

```javascript
const result = await client.system().ping();
// { version: "1.0.0", timestamp: "2024-01-01T00:00:00Z", status: "ok" }
```

```rust
let result = client.system().ping().await?;
// Ping { version: Some("1.0.0"), timestamp: Some("..."), status: Some("ok") }
```

**Returns:** `Ping`

| Field | Type | Description |
|-------|------|-------------|
| `version` | `string?` | API version |
| `timestamp` | `string?` | Server timestamp (RFC3339) |
| `status` | `string?` | Service status |

---

### getRedirectUrl / get_redirect_url

Get redirect URL for a destination identifier.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `destination` | `string` | Destination identifier (e.g., "openapi") |

```javascript
const url = await client.system().getRedirectUrl('openapi');
// "https://test.jacktrip.com/api/redirect/openapi"
```

```rust
let url = client.system().get_redirect_url("openapi").await?;
```

**Returns:** `string` - The redirect URL or final destination after following redirects

---

### getMyIp / get_my_ip

Get the client's public IP address as seen by the server.

**Authentication:** None required

```javascript
const ip = await client.system().getMyIp();
// "203.0.113.1"
```

```rust
let ip = client.system().get_my_ip().await?;
// "203.0.113.1"
```

**Returns:** `string`

---

### listRegions / list_regions

List all available cloud regions for hosting studios.

**Authentication:** None required

```javascript
const regions = await client.system().listRegions();
// [{ id: "us-west-2", label: "US West", ... }, ...]
```

```rust
let regions = client.system().list_regions().await?;
// Vec<Region>
```

**Returns:** `Region[]`

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string?` | Region identifier |
| `label` | `string?` | Human-readable name |
| `group` | `string?` | Geographic group |
| `provider` | `string?` | Cloud provider |
| `region` | `string?` | Provider region code |
| `latitude` | `number?` | Latitude coordinate |
| `longitude` | `number?` | Longitude coordinate |
| `active` | `boolean?` | Whether region is active |
| `instanceTypes` | `InstanceType[]?` | Available instance types |

---

### getRegion / get_region

Get details for a specific region.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `region` | `string` | Region identifier |

```javascript
const region = await client.system().getRegion('us-west-2');
```

```rust
let region = client.system().get_region("us-west-2").await?;
```

**Returns:** `Region`

---

### collectAnalytics / collect_analytics

Submit an analytics event.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `event` | `AnalyticsEvent` | Event data |

```javascript
await client.system().collectAnalytics({
  event: 'page_view',
  properties: { page: '/studios' },
  anonymousId: 'abc123'
});
```

```rust
client.system().collect_analytics(&AnalyticsEvent {
    event: Some("page_view".to_string()),
    properties: Some(json!({ "page": "/studios" })),
    anonymous_id: Some("abc123".to_string()),
    ..Default::default()
}).await?;
```

**Returns:** `void` / `()`

**AnalyticsEvent Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `event` | `string?` | Event name |
| `properties` | `object?` | Event properties |
| `userId` | `string?` | User ID (if authenticated) |
| `anonymousId` | `string?` | Anonymous ID |
| `timestamp` | `string?` | Event timestamp (RFC3339) |

