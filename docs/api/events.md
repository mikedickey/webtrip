# Events API

JackTrip Radio upcoming events and broadcasts.

## Access

```javascript
const eventsApi = client.events();
```

```rust
let events_api = client.events();
```

## Methods

### listEvents / list_events

List all public upcoming events.

**Authentication:** None required

```javascript
const events = await client.events().listEvents();
```

```rust
let events = client.events().list_events().await?;
```

**Returns:** `PublicUpcomingEvent[]`

---

### listEventsPaginated / list_events_paginated

List events with pagination.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `page` | `number?` | Page number |
| `limit` | `number?` | Items per page |

```javascript
const result = await client.events().listEventsPaginated(1, 20);
```

```rust
let result = client.events().list_events_paginated(Some(1), Some(20)).await?;
```

**Returns:** `PaginatedEvents`

---

### getEvent / get_event

Get a public event by ID.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `eventId` | `string` | Event ID |

```javascript
const events = await client.events().getEvent('event123');
```

```rust
let events = client.events().get_event("event123").await?;
```

**Returns:** `PublicUpcomingEvent[]`

---

### getEventChannel / get_event_channel

Get the radio channel for an event.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `eventId` | `string` | Event ID |

```javascript
const channel = await client.events().getEventChannel('event123');
```

```rust
let channel = client.events().get_event_channel("event123").await?;
```

**Returns:** `StreamInfo`

---

### getSimilarEvents / get_similar_events

Get events similar to a given event.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `eventId` | `string` | Event ID |

```javascript
const similar = await client.events().getSimilarEvents('event123');
```

```rust
let similar = client.events().get_similar_events("event123").await?;
```

**Returns:** `PublicUpcomingEvent[]`

---

### getEventLive / get_event_live

Get the live stream URL for an active event. The server returns a `400` error when the
event is not currently active (before its start time or after its end time).

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `eventId` | `string` | Event ID |

```javascript
const live = await client.events().getEventLive('event123');
// { redirect: "https://live.example.com/stream" }
```

```rust
let live = client.events().get_event_live("event123").await?;
```

**Returns:** `Redirect`

| Field | Type | Description |
|-------|------|-------------|
| `redirect` | `string?` | URL of the active live stream |

---

### listStudioEvents / list_studio_events

List events for a specific studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const events = await client.events().listStudioEvents('studio123');
```

```rust
let events = client.events().list_studio_events("studio123").await?;
```

**Returns:** `UpcomingEvent[]`

---

### getStudioEvent / get_studio_event

Get a specific event for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `eventId` | `string` | Event ID |

```javascript
const event = await client.events().getStudioEvent('studio123', 'event456');
```

```rust
let event = client.events().get_studio_event("studio123", "event456").await?;
```

**Returns:** `UpcomingEvent`

---

### createStudioEvent / create_studio_event

Create a new event for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `event` | `UpcomingEvent` | Event details |

```javascript
const event = await client.events().createStudioEvent('studio123', {
  title: 'Live Session',
  startTime: '2024-01-15T20:00:00Z',
  duration: 3600
});
```

```rust
let event = client.events().create_studio_event("studio123", &new_event).await?;
```

**Returns:** `UpcomingEvent`

---

### updateStudioEvent / update_studio_event

Update an event for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `eventId` | `string` | Event ID |
| `event` | `UpcomingEvent` | Updated event |

```javascript
const updated = await client.events().updateStudioEvent('studio123', 'event456', {
  title: 'Updated Title'
});
```

```rust
let updated = client.events().update_studio_event("studio123", "event456", &event).await?;
```

**Returns:** `UpcomingEvent`

---

### deleteStudioEvent / delete_studio_event

Delete an event for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `eventId` | `string` | Event ID |

```javascript
await client.events().deleteStudioEvent('studio123', 'event456');
```

```rust
client.events().delete_studio_event("studio123", "event456").await?;
```

**Returns:** `void` / `()`

