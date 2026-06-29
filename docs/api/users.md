# Users API

User profile management, notifications, referrals, and related operations.

## Access

```javascript
const usersApi = client.users();
```

```rust
let users_api = client.users();
```

## Methods

### getCurrentUser / get_current_user

Get the currently authenticated user's profile.

**Authentication:** Required

```javascript
const user = await client.users().getCurrentUser();
```

```rust
let user = client.users().get_current_user().await?;
```

**Returns:** `User`

---

### getUser / get_user

Get a user by ID.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const user = await client.users().getUser('user123');
```

```rust
let user = client.users().get_user("user123").await?;
```

**Returns:** `User`

---

### updateUser / update_user

Update a user's metadata.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `metadata` | `UserMetadata` | Updated metadata |

```javascript
const updated = await client.users().updateUser('user123', {
  displayName: 'New Name',
  bio: 'Updated bio'
});
```

```rust
let updated = client.users().update_user("user123", &metadata).await?;
```

**Returns:** `UserMetadata`

---

### deleteUser / delete_user

Delete a user account.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
await client.users().deleteUser('user123');
```

```rust
client.users().delete_user("user123").await?;
```

**Returns:** `void` / `()`

---

### getUserRegions / get_user_regions

Get all regions available to a user.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const regions = await client.users().getUserRegions('user123');
```

```rust
let regions = client.users().get_user_regions("user123").await?;
```

**Returns:** `Region[]`

---

### getNotifications / get_notifications

Get a user's notifications.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const notifications = await client.users().getNotifications('user123');
```

```rust
let notifications = client.users().get_notifications("user123").await?;
```

**Returns:** `Notification[]`

---

### getConversations / get_conversations

Get a user's conversations.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const conversations = await client.users().getConversations('user123');
```

```rust
let conversations = client.users().get_conversations("user123").await?;
```

**Returns:** `Conversation[]`

---

### getConversation / get_conversation

Get a specific conversation between the user and a studio, identified by the stream ID.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `streamId` | `string` | Stream/channel ID identifying the conversation |

```javascript
const conversation = await client.users().getConversation('user123', 'stream456');
```

```rust
let conversation = client.users().get_conversation("user123", "stream456").await?;
```

**Returns:** `Conversation`

---

### getHubspotToken / get_hubspot_token

Get a HubSpot visitor identification token for the user. Used to authenticate with the
HubSpot Conversations API on the frontend. Users may only fetch their own token.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const result = await client.users().getHubspotToken('user123');
// { token: "hs-visitor-..." }
```

```rust
let result = client.users().get_hubspot_token("user123").await?;
```

**Returns:** `HubSpotToken`

| Field | Type | Description |
|-------|------|-------------|
| `token` | `string?` | HubSpot visitor identification token |

---

### getUnreadMessagesCount / get_unread_messages_count

Get the count of unread messages for a user.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const result = await client.users().getUnreadMessagesCount('user123');
// { count: 5 }
```

```rust
let result = client.users().get_unread_messages_count("user123").await?;
```

**Returns:** `UnreadMessagesResponse`

---

### getReferrals / get_referrals

Get a user's referrals.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const referrals = await client.users().getReferrals('user123');
```

```rust
let referrals = client.users().get_referrals("user123").await?;
```

**Returns:** `Referral[]`

---

### createReferral / create_referral

Create a new referral code.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const referral = await client.users().createReferral('user123');
```

```rust
let referral = client.users().create_referral("user123").await?;
```

**Returns:** `Referral`

---

### getUserChannels / get_user_channels

Get paginated channels the user is a member of.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `page` | `number?` | Page number |
| `limit` | `number?` | Items per page |

```javascript
const channels = await client.users().getUserChannels('user123', 1, 20);
```

```rust
let channels = client.users().get_user_channels("user123", Some(1), Some(20)).await?;
```

**Returns:** `PaginatedChannels`

---

### getUserFollows / get_user_follows

Get paginated channels the user follows.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `page` | `number?` | Page number |
| `limit` | `number?` | Items per page |

```javascript
const follows = await client.users().getUserFollows('user123', 1, 20);
```

```rust
let follows = client.users().get_user_follows("user123", Some(1), Some(20)).await?;
```

**Returns:** `PaginatedChannels`

