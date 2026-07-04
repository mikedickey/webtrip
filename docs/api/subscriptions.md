# Subscriptions API

Studio **memberships**. A "subscription" here is **not** a billing plan (see
[Billing](./billing.md) for Stripe billing) — it represents a user who is a
member of a studio. The endpoints span two URL roots
(`/users/{userId}/subscriptions` and `/studios/{studioId}/subscriptions*`) but
model the single membership concept.

## Access

```javascript
const subscriptionsApi = client.subscriptions();
```

```rust
let subscriptions_api = client.subscriptions();
```

## Methods

### listUserSubscriptions / list_user_subscriptions

List the studios a user is a member of (`GET /users/{userId}/subscriptions`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const memberships = await client.subscriptions().listUserSubscriptions('user123');
```

```rust
let memberships = client.subscriptions().list_user_subscriptions("user123").await?;
```

**Returns:** `Subscription[]`

---

### listStudioSubscriptions / list_studio_subscriptions

List a studio's members (`GET /studios/{studioId}/subscriptions`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const members = await client.subscriptions().listStudioSubscriptions('studio123');
```

```rust
let members = client.subscriptions().list_studio_subscriptions("studio123").await?;
```

**Returns:** `Subscription[]`

---

### createSubscription / create_subscription

Add a member to a studio (`POST /studios/{studioId}/subscriptions`). `inviteKey`
is only required for private studios.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `request` | `CreateSubscriptionRequest` | Membership details |

```javascript
const membership = await client.subscriptions().createSubscription('studio123', {
  userId: 'user456',
  inviteKey: 'abc123'
});
```

```rust
let request = CreateSubscriptionRequest {
    user_id: Some("user456".into()),
    invite_key: Some("abc123".into()),
    ..Default::default()
};
let membership = client.subscriptions().create_subscription("studio123", &request).await?;
```

**Returns:** `Subscription`

**CreateSubscriptionRequest Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `serverId` | `string?` | Studio ID to subscribe to |
| `userId` | `string?` | User ID of the subscriber |
| `inviteKey` | `string?` | Invite key (required for private studios) |

---

### getSubscription / get_subscription

Describe a single membership between a studio and a user
(`GET /studios/{studioId}/subscriptions/{userId}`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `userId` | `string` | Member's user ID |

```javascript
const membership = await client.subscriptions().getSubscription('studio123', 'user456');
```

```rust
let membership = client.subscriptions().get_subscription("studio123", "user456").await?;
```

**Returns:** `Subscription`

---

### updateSubscription / update_subscription

Update a membership, e.g. the admin flag or status
(`PUT /studios/{studioId}/subscriptions/{userId}`). The updated membership is
sent as the request body, mirroring the studio/device update endpoints.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `userId` | `string` | Member's user ID |
| `subscription` | `Subscription` | Updated membership |

```javascript
const updated = await client.subscriptions().updateSubscription('studio123', 'user456', {
  admin: true,
  status: 'Active'
});
```

```rust
let updated = client.subscriptions().update_subscription("studio123", "user456", &subscription).await?;
```

**Returns:** `Subscription`

---

### deleteSubscription / delete_subscription

Remove a member from a studio
(`DELETE /studios/{studioId}/subscriptions/{userId}`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `userId` | `string` | Member's user ID |

```javascript
await client.subscriptions().deleteSubscription('studio123', 'user456');
```

```rust
client.subscriptions().delete_subscription("studio123", "user456").await?;
```

**Returns:** `void` / `()`

---

## Types

### Subscription

A studio membership linking a user to a studio. Note the mixed wire casing:
`user_id` and `updated_at` are snake_case while `serverId` is camelCase.

| Field | Type | Description |
|-------|------|-------------|
| `user_id` | `string?` | Member's user ID |
| `name` | `string?` | Member's display name |
| `nickname` | `string?` | Member's nickname |
| `picture` | `string?` | URL to the member's profile picture |
| `email` | `string?` | Member's email address |
| `updated_at` | `string?` | RFC3339 timestamp of the last user-profile update |
| `serverId` | `string?` | Studio ID |
| `admin` | `boolean?` | Whether the member is a studio admin |
| `status` | `string?` | Membership status (`Active`, `Deleted`) |
| `createdAt` | `string?` | Membership creation timestamp (RFC3339) |
| `updatedAt` | `string?` | Membership update timestamp (RFC3339) |
