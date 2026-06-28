# Streams API

JackTrip Radio live streams and channel management.

## Access

```javascript
const streamsApi = client.streams();
```

```rust
let streams_api = client.streams();
```

## Methods

### listStreams / list_streams

List all public, active broadcasts.

**Authentication:** None required

```javascript
const streams = await client.streams().listStreams();
```

```rust
let streams = client.streams().list_streams().await?;
```

**Returns:** `StreamInfo[]`

---

### searchStreams / search_streams

Search for broadcasts.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `query` | `StreamSearchQuery` | Search filters (`q`, `lookingFor`, `skillLevel`, `instrument`, `genre`, `region`, `page`, `limit`) |

```javascript
const results = await client.streams().searchStreams({ q: 'jazz', genre: 'jazz', page: 1, limit: 20 });
results.results.forEach(s => console.log(s.name));
```

```rust
let query = StreamSearchQuery { q: Some("jazz".into()), ..Default::default() };
let results = client.streams().search_streams(&query).await?;
```

**Returns:** `PaginatedStreamSearchResults` (`{ _meta, results: StreamInfoSearchResult[] }`)

---

### getStream / get_stream

Get a broadcast by ID.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |

```javascript
const stream = await client.streams().getStream('stream123');
```

```rust
let stream = client.streams().get_stream("stream123").await?;
```

**Returns:** `StreamInfoWithEngagement`

---

### followStream / follow_stream

Follow a broadcast.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |

```javascript
await client.streams().followStream('stream123');
```

```rust
client.streams().follow_stream("stream123").await?;
```

**Returns:** `void` / `()`

---

### unfollowStream / unfollow_stream

Unfollow a broadcast.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |

```javascript
await client.streams().unfollowStream('stream123');
```

```rust
client.streams().unfollow_stream("stream123").await?;
```

**Returns:** `void` / `()`

---

### listChannels / list_channels

List all public channels.

**Authentication:** None required

```javascript
const channels = await client.streams().listChannels();
```

```rust
let channels = client.streams().list_channels().await?;
```

**Returns:** `StreamInfo[]`

---

### listChannelsPaginated / list_channels_paginated

List channels with pagination.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `page` | `number?` | Page number |
| `limit` | `number?` | Items per page |

```javascript
const result = await client.streams().listChannelsPaginated(1, 20);
```

```rust
let result = client.streams().list_channels_paginated(Some(1), Some(20)).await?;
```

**Returns:** `PaginatedChannels`

---

### getStreamChat / get_stream_chat

Get chat session for a broadcast.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |
| `chatId` | `string` | Chat ID |

```javascript
const chat = await client.streams().getStreamChat('stream123', 'chat456');
```

```rust
let chat = client.streams().get_stream_chat("stream123", "chat456").await?;
```

**Returns:** `ChatSession`

---

### getStreamConversations / get_stream_conversations

Get all conversations for a stream.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |

```javascript
const conversations = await client.streams().getStreamConversations('stream123');
```

```rust
let conversations = client.streams().get_stream_conversations("stream123").await?;
```

**Returns:** `Conversation[]`

---

### getStreamConversation / get_stream_conversation

Get a specific conversation.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |
| `userId` | `string` | User ID |

```javascript
const conversation = await client.streams().getStreamConversation('stream123', 'user456');
```

```rust
let conversation = client.streams().get_stream_conversation("stream123", "user456").await?;
```

**Returns:** `Conversation`

---

### getConversationMessages / get_conversation_messages

Get messages in a conversation.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |
| `userId` | `string` | User ID |

```javascript
const messages = await client.streams().getConversationMessages('stream123', 'user456');
```

```rust
let messages = client.streams().get_conversation_messages("stream123", "user456").await?;
```

**Returns:** `Message[]`

---

### sendMessage / send_message

Send a message in a conversation.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |
| `userId` | `string` | User ID |
| `message` | `SendMessageRequest` | Message content |

```javascript
const message = await client.streams().sendMessage('stream123', 'user456', {
  content: 'Hello!',
  type: 'text'
});
```

```rust
let message = client.streams().send_message("stream123", "user456", &msg).await?;
```

**Returns:** `Message`

---

### getStudioStream / get_studio_stream

Get the stream for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const stream = await client.streams().getStudioStream('studio123');
```

```rust
let stream = client.streams().get_studio_stream("studio123").await?;
```

**Returns:** `LiveStream`

---

### createStudioStream / create_studio_stream

Create or reset a stream for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `stream` | `LiveStream` | Stream configuration |

```javascript
const stream = await client.streams().createStudioStream('studio123', {
  title: 'My Live Stream'
});
```

```rust
let stream = client.streams().create_studio_stream("studio123", &config).await?;
```

**Returns:** `LiveStream`

---

### updateStudioStream / update_studio_stream

Update a studio's stream.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `stream` | `LiveStream` | Updated configuration |

```javascript
const stream = await client.streams().updateStudioStream('studio123', {
  title: 'Updated Title'
});
```

```rust
let stream = client.streams().update_studio_stream("studio123", &config).await?;
```

**Returns:** `LiveStream`

---

### activateStudioStream / activate_studio_stream

Activate or deactivate a studio stream.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `opts` | `ActivationRequestOpts` | Activation options |

```javascript
const stream = await client.streams().activateStudioStream('studio123', {
  active: true
});
```

```rust
let stream = client.streams().activate_studio_stream("studio123", &opts).await?;
```

**Returns:** `LiveStream`

---

### getSimulcastDestinations / get_simulcast_destinations

Get simulcast destinations for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const destinations = await client.streams().getSimulcastDestinations('studio123');
```

```rust
let destinations = client.streams().get_simulcast_destinations("studio123").await?;
```

**Returns:** `SimulcastDestination[]`

---

### updateSimulcastDestination / update_simulcast_destination

Add or update a simulcast destination.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `destination` | `string` | Destination name |
| `config` | `SimulcastDestination` | Destination config |

```javascript
const dest = await client.streams().updateSimulcastDestination('studio123', 'youtube', {
  rtmpUrl: 'rtmp://...',
  streamKey: 'abc123'
});
```

```rust
let dest = client.streams().update_simulcast_destination("studio123", "youtube", &config).await?;
```

**Returns:** `SimulcastDestination`

---

### deleteSimulcastDestination / delete_simulcast_destination

Remove a simulcast destination.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `destination` | `string` | Destination name |

```javascript
await client.streams().deleteSimulcastDestination('studio123', 'youtube');
```

```rust
client.streams().delete_simulcast_destination("studio123", "youtube").await?;
```

**Returns:** `void` / `()`

