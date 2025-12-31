# Recordings API

JackTrip Radio recordings management.

## Access

```javascript
const recordingsApi = client.recordings();
```

```rust
let recordings_api = client.recordings();
```

## Methods

### listRecordings / list_recordings

List all public recordings.

**Authentication:** None required

```javascript
const recordings = await client.recordings().listRecordings();
```

```rust
let recordings = client.recordings().list_recordings().await?;
```

**Returns:** `RecordingMetadata[]`

---

### listRecordingsPaginated / list_recordings_paginated

List recordings with pagination.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `page` | `number?` | Page number |
| `limit` | `number?` | Items per page |
| `following` | `boolean?` | Only followed channels |

```javascript
const result = await client.recordings().listRecordingsPaginated(1, 20, true);
```

```rust
let result = client.recordings().list_recordings_paginated(Some(1), Some(20), Some(true)).await?;
```

**Returns:** `PaginatedRecordings`

---

### getRecording / get_recording

Get a recording by ID.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `recordingId` | `string` | Recording ID |

```javascript
const recording = await client.recordings().getRecording('rec123');
```

```rust
let recording = client.recordings().get_recording("rec123").await?;
```

**Returns:** `PersonalizedRecording`

---

### getSimilarRecordings / get_similar_recordings

Get recordings similar to a given recording.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `recordingId` | `string` | Recording ID |

```javascript
const similar = await client.recordings().getSimilarRecordings('rec123');
```

```rust
let similar = client.recordings().get_similar_recordings("rec123").await?;
```

**Returns:** `RecordingMetadata[]`

---

### likeRecording / like_recording

Like a recording.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `recordingId` | `string` | Recording ID |

```javascript
await client.recordings().likeRecording('rec123');
```

```rust
client.recordings().like_recording("rec123").await?;
```

**Returns:** `void` / `()`

---

### unlikeRecording / unlike_recording

Unlike a recording.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `recordingId` | `string` | Recording ID |

```javascript
await client.recordings().unlikeRecording('rec123');
```

```rust
client.recordings().unlike_recording("rec123").await?;
```

**Returns:** `void` / `()`

---

### getStreamRecordings / get_stream_recordings

Get recordings for a stream/channel.

**Authentication:** None required

| Parameter | Type | Description |
|-----------|------|-------------|
| `streamId` | `string` | Stream ID |

```javascript
const recordings = await client.recordings().getStreamRecordings('stream123');
```

```rust
let recordings = client.recordings().get_stream_recordings("stream123").await?;
```

**Returns:** `RecordingMetadata[]`

---

### getStudioRecordings / get_studio_recordings

Get all recordings for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const recordings = await client.recordings().getStudioRecordings('studio123');
```

```rust
let recordings = client.recordings().get_studio_recordings("studio123").await?;
```

**Returns:** `ServerRecording[]`

---

### getStudioRecordingsPaginated / get_studio_recordings_paginated

Get paginated recordings for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `page` | `number?` | Page number |
| `limit` | `number?` | Items per page |

```javascript
const result = await client.recordings().getStudioRecordingsPaginated('studio123', 1, 20);
```

```rust
let result = client.recordings().get_studio_recordings_paginated("studio123", Some(1), Some(20)).await?;
```

**Returns:** `PaginatedRecordings`

---

### getStudioRecording / get_studio_recording

Get a specific recording for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `recordingId` | `string` | Recording ID |

```javascript
const recording = await client.recordings().getStudioRecording('studio123', 'rec456');
```

```rust
let recording = client.recordings().get_studio_recording("studio123", "rec456").await?;
```

**Returns:** `ServerRecording`

---

### updateStudioRecording / update_studio_recording

Update a recording for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `recordingId` | `string` | Recording ID |
| `metadata` | `RecordingMetadata` | Updated metadata |

```javascript
const updated = await client.recordings().updateStudioRecording('studio123', 'rec456', {
  title: 'Updated Title',
  description: 'New description'
});
```

```rust
let updated = client.recordings().update_studio_recording("studio123", "rec456", &metadata).await?;
```

**Returns:** `ServerRecording`

---

### deleteStudioRecording / delete_studio_recording

Delete a recording for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `recordingId` | `string` | Recording ID |

```javascript
await client.recordings().deleteStudioRecording('studio123', 'rec456');
```

```rust
client.recordings().delete_studio_recording("studio123", "rec456").await?;
```

**Returns:** `void` / `()`

---

### getRecordingStems / get_recording_stems

Get stem information for a recording (individual audio tracks).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `recordingId` | `string` | Recording ID |

```javascript
const stems = await client.recordings().getRecordingStems('studio123', 'rec456');
```

```rust
let stems = client.recordings().get_recording_stems("studio123", "rec456").await?;
```

**Returns:** `StemInfo[]`

---

### getUserRecordings / get_user_recordings

Get all recordings for a user.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const recordings = await client.recordings().getUserRecordings('user123');
```

```rust
let recordings = client.recordings().get_user_recordings("user123").await?;
```

**Returns:** `ServerRecording[]`

---

### getUserRecordingsPaginated / get_user_recordings_paginated

Get paginated recordings for a user.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `page` | `number?` | Page number |
| `limit` | `number?` | Items per page |

```javascript
const result = await client.recordings().getUserRecordingsPaginated('user123', 1, 20);
```

```rust
let result = client.recordings().get_user_recordings_paginated("user123", Some(1), Some(20)).await?;
```

**Returns:** `PaginatedRecordings`

---

### getRecordingsQuota / get_recordings_quota

Get recordings quota for a user.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const quota = await client.recordings().getRecordingsQuota('user123');
```

```rust
let quota = client.recordings().get_recordings_quota("user123").await?;
```

**Returns:** `RecordingsQuota`

