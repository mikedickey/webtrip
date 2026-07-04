# Backing Tracks API

Upload, list, and manage backing-track audio files stored for a studio
(`/studios/{studioId}/tracks*`).

> **Note:** `GET /studios/{studioId}/tracks/{trackId}/download` is intentionally
> not exposed. Per the OpenAPI spec it is authenticated with VS-agent
> `APIPrefix`/`APISecret` credentials rather than the user JWT this client
> carries, making it a VS-agent (out-of-scope) endpoint.

## Access

```javascript
const tracksApi = client.tracks();
```

```rust
let tracks_api = client.tracks();
```

## Methods

### listTracks / list_tracks

List backing tracks for a studio, paginated
(`GET /studios/{studioId}/tracks`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `page` | `number?` | Page number |
| `limit` | `number?` | Items per page |

```javascript
const result = await client.tracks().listTracks('studio123', 1, 20);
result.results.forEach(t => console.log(t.name));
```

```rust
let result = client.tracks().list_tracks("studio123", Some(1), Some(20)).await?;
```

**Returns:** `PaginatedBackingTracks` (`{ _meta, results: BackingTrack[] }`)

---

### uploadTrack / upload_track

Upload a new backing track (WAV, MP3, or FLAC) as `multipart/form-data`
(`POST /studios/{studioId}/tracks`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `fileName` | `string` | Original file name |
| `data` | `Uint8Array` / `Vec<u8>` | Raw audio file bytes |

```javascript
const track = await client.tracks().uploadTrack('studio123', 'bassline.wav', wavBytes);
```

```rust
let track = client.tracks().upload_track("studio123", "bassline.wav", wav_bytes).await?;
```

**Returns:** `BackingTrack`

---

### getTrack / get_track

Get a backing track by ID (`GET /studios/{studioId}/tracks/{trackId}`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `trackId` | `string` | Track ID |

```javascript
const track = await client.tracks().getTrack('studio123', 'track456');
```

```rust
let track = client.tracks().get_track("studio123", "track456").await?;
```

**Returns:** `BackingTrack`

---

### updateTrack / update_track

Update a backing track's metadata / mix settings
(`PUT /studios/{studioId}/tracks/{trackId}`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `trackId` | `string` | Track ID |
| `update` | `TrackUpdateRequest` | Fields to update |

```javascript
const updated = await client.tracks().updateTrack('studio123', 'track456', {
  name: 'Bassline (final)',
  volume: 80,
  mute: false
});
```

```rust
let update = TrackUpdateRequest { name: Some("Bassline (final)".into()), volume: Some(80), ..Default::default() };
let updated = client.tracks().update_track("studio123", "track456", &update).await?;
```

**Returns:** `BackingTrack`

**TrackUpdateRequest Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `name` | `string?` | Track display name |
| `volume` | `number?` | Volume level (0-100) |
| `pan` | `number?` | Pan position (-100 to 100) |
| `mute` | `boolean?` | Whether the track is muted |
| `solo` | `boolean?` | Whether the track is soloed |

---

### deleteTrack / delete_track

Delete a backing track (`DELETE /studios/{studioId}/tracks/{trackId}`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `trackId` | `string` | Track ID |

```javascript
await client.tracks().deleteTrack('studio123', 'track456');
```

```rust
client.tracks().delete_track("studio123", "track456").await?;
```

**Returns:** `void` / `()`

---

## Types

### BackingTrack

A backing-track file stored for a studio.

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string?` | Track ID |
| `serverId` | `string?` | Studio ID |
| `ownerId` | `string?` | User ID of the track owner |
| `location` | `string?` | GCS location of the backing-track file |
| `name` | `string?` | Track display name |
| `duration` | `number?` | Track duration in seconds |
| `status` | `number?` | Track status (0=ready, 1=deleting) |
| `createdAt` | `string?` | Upload timestamp (RFC3339) |
| `updatedAt` | `string?` | Last modification timestamp (RFC3339) |
