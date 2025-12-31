# Studios API

Virtual studio management, configuration, and related operations.

## Access

```javascript
const studiosApi = client.studios();
```

```rust
let studios_api = client.studios();
```

## Methods

### listStudios / list_studios

List all studios for the authenticated user.

**Authentication:** Required

```javascript
const studios = await client.studios().listStudios();
```

```rust
let studios = client.studios().list_studios().await?;
```

**Returns:** `Studio[]`

---

### createStudio / create_studio

Create a new studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studio` | `Studio` | Studio configuration |

```javascript
const studio = await client.studios().createStudio({
  name: 'My Studio',
  region: 'us-west-2'
});
```

```rust
let studio = client.studios().create_studio(&new_studio).await?;
```

**Returns:** `Studio`

---

### getStudio / get_studio

Get a studio by ID.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const studio = await client.studios().getStudio('studio123');
```

```rust
let studio = client.studios().get_studio("studio123").await?;
```

**Returns:** `Studio`

---

### updateStudio / update_studio

Update a studio's configuration.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `studio` | `Studio` | Updated configuration |

```javascript
const updated = await client.studios().updateStudio('studio123', {
  name: 'Updated Name'
});
```

```rust
let updated = client.studios().update_studio("studio123", &studio).await?;
```

**Returns:** `Studio`

---

### deleteStudio / delete_studio

Delete a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
await client.studios().deleteStudio('studio123');
```

```rust
client.studios().delete_studio("studio123").await?;
```

**Returns:** `void` / `()`

---

### extendStudio / extend_studio

Extend a studio's expiration time.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const studio = await client.studios().extendStudio('studio123');
```

```rust
let studio = client.studios().extend_studio("studio123").await?;
```

**Returns:** `Studio`

---

### getAccessSettings / get_access_settings

Get access settings for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const settings = await client.studios().getAccessSettings('studio123');
```

```rust
let settings = client.studios().get_access_settings("studio123").await?;
```

**Returns:** `AccessSettings`

---

### updateAccessSettings / update_access_settings

Update access settings for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `settings` | `AccessSettings` | New settings |

```javascript
const updated = await client.studios().updateAccessSettings('studio123', {
  isPublic: true,
  requireApproval: false
});
```

```rust
let updated = client.studios().update_access_settings("studio123", &settings).await?;
```

**Returns:** `AccessSettings`

---

### getMixer / get_mixer

Get the mixer configuration for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const mixer = await client.studios().getMixer('studio123');
```

```rust
let mixer = client.studios().get_mixer("studio123").await?;
```

**Returns:** `Mixer`

---

### updateMixer / update_mixer

Update the mixer configuration for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `mixer` | `Mixer` | New mixer configuration |

```javascript
const updated = await client.studios().updateMixer('studio123', mixer);
```

```rust
let updated = client.studios().update_mixer("studio123", &mixer).await?;
```

**Returns:** `Mixer`

---

### listMixers / list_mixers

Get all mixers.

**Authentication:** Required

```javascript
const mixers = await client.studios().listMixers();
```

```rust
let mixers = client.studios().list_mixers().await?;
```

**Returns:** `Mixer[]`

---

### getLivekitToken / get_livekit_token

Get a LiveKit token for the studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const { token, url } = await client.studios().getLivekitToken('studio123');
```

```rust
let response = client.studios().get_livekit_token("studio123").await?;
```

**Returns:** `LiveKitTokenResponse`

| Field | Type | Description |
|-------|------|-------------|
| `token` | `string?` | LiveKit access token |
| `url` | `string?` | LiveKit server URL |

---

### sendInvite / send_invite

Send an invite for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `invite` | `InviteRequest` | Invite details |

```javascript
await client.studios().sendInvite('studio123', {
  email: 'friend@example.com',
  message: 'Join my studio!'
});
```

```rust
client.studios().send_invite("studio123", &invite).await?;
```

**Returns:** `void` / `()`

---

### submitFeedback / submit_feedback

Submit feedback for a studio session.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `feedback` | `FeedbackRequest` | Feedback data |

```javascript
await client.studios().submitFeedback('studio123', {
  rating: 5,
  comment: 'Great session!'
});
```

```rust
client.studios().submit_feedback("studio123", &feedback).await?;
```

**Returns:** `void` / `()`

---

### getChat / get_chat

Get chat session for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `chatId` | `string` | Chat session ID |

```javascript
const chat = await client.studios().getChat('studio123', 'chat456');
```

```rust
let chat = client.studios().get_chat("studio123", "chat456").await?;
```

**Returns:** `ChatSession`

---

### getParticipants / get_participants

Get participants in a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const participants = await client.studios().getParticipants('studio123');
```

```rust
let participants = client.studios().get_participants("studio123").await?;
```

**Returns:** `Participant[]`

---

### getSession / get_session

Get the current session for a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const session = await client.studios().getSession('studio123');
```

```rust
let session = client.studios().get_session("studio123").await?;
```

**Returns:** `Session`

---

## Types

### Studio

A JackTrip Virtual Studio instance.

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string?` | Studio ID |
| `ownerId` | `string?` | Owner's user ID |
| `cloudId` | `string?` | Cloud instance identifier |
| `sessionId` | `string?` | Active session identifier |
| `streamId` | `string?` | Associated stream identifier (for broadcasting) |
| `unlistedStreamId` | `string?` | Unlisted stream identifier |
| `chatId` | `string?` | Chat room identifier |
| `region` | `string?` | Cloud region identifier |
| `size` | `string?` | Instance size/type |
| `bannerURL` | `string?` | Banner image URL |
| `status` | `ResourceStatus?` | Current status (Starting, Ready, Disabled, Deleting) |
| `period` | `Period?` | Audio frame period (16, 32, 64, 128, 256, 512, 1024, 2048) |
| `queueBuffer` | `QueueBuffer?` | Jitter buffer size |
| `bufferStrategy` | `BufferStrategy?` | Buffer strategy (1=Standard, 2=AutoAdjust, 3=Broadcast) |
| `mixBranch` | `string?` | SuperCollider mixer branch name |
| `mixCode` | `string?` | Custom SuperCollider mixer code |
| `broadcast` | `BroadcastVisibility?` | Broadcast visibility (0=Off, 1=Private, 2=Public) |
| `maxMusicians` | `number?` | Maximum number of musicians allowed |
| `expiresAt` | `string?` | Expiration timestamp (RFC3339) |
| `createdAt` | `string?` | Creation timestamp (RFC3339) |
| `updatedAt` | `string?` | Last update timestamp (RFC3339) |
| `type` | `StudioType?` | Studio type ("JackTrip" or "JackTrip+Jamulus") |
| `name` | `string?` | Studio display name |
| `serverHost` | `string?` | Studio hostname/IP address |
| `serverPort` | `number?` | Studio port number |
| `sampleRate` | `SampleRate?` | Audio sample rate (44100, 48000, 88200, 96000) |
| `public` | `boolean?` | Whether the studio is publicly visible |
| `stereo` | `boolean?` | Whether stereo audio is enabled |
| `loopback` | `boolean?` | Whether loopback audio is enabled |
| `enabled` | `boolean?` | Whether the studio is currently active/enabled |
| `admin` | `boolean?` | Whether the current user is an admin of this studio |
| `owner` | `boolean?` | Whether the current user is the owner of this studio |
| `subStatus` | `string?` | Subscription status (Active, Deleted) |

### AccessSettings

Studio access control settings.

| Field | Type | Description |
|-------|------|-------------|
| `passwordProtected` | `boolean?` | Whether the studio requires a password |
| `password` | `string?` | Studio access password (write-only) |
| `allowGuests` | `boolean?` | Whether to allow anonymous/guest access |
| `maxGuests` | `number?` | Maximum number of guests allowed |
| `allowedUsers` | `string[]?` | Allowed user IDs (if restricted) |

### Mixer

Studio mixer configuration.

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string?` | Mixer ID |
| `name` | `string?` | Mixer name |
| `description` | `string?` | Mixer description |
| `branch` | `string?` | SuperCollider code branch |
| `code` | `string?` | Custom SuperCollider code |
| `preset` | `boolean?` | Whether this is a system preset |

### Participant

A participant in a studio session.

| Field | Type | Description |
|-------|------|-------------|
| `userId` | `string?` | Participant's user ID |
| `name` | `string?` | Participant's display name |
| `deviceId` | `string?` | Device ID (for JackTrip devices) |
| `muted` | `boolean?` | Whether the participant is muted |
| `volume` | `number?` | Participant's volume level (0-100) |
| `joinedAt` | `string?` | Join timestamp (RFC3339) |

### Enums

#### StudioType

| Value | Description |
|-------|-------------|
| `"JackTrip"` | JackTrip audio engine only |
| `"JackTrip+Jamulus"` | JackTrip with Jamulus bridge |

#### SampleRate

| Value | Description |
|-------|-------------|
| `44100` | 44.1 kHz (CD quality) |
| `48000` | 48 kHz (professional audio) |
| `88200` | 88.2 kHz (high resolution) |
| `96000` | 96 kHz (high resolution) |

