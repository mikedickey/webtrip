# Billing API

Subscription and payment management.

## Access

```javascript
const billingApi = client.billing();
```

```rust
let billing_api = client.billing();
```

## Methods

### getPlans / get_plans

Get available subscription plans.

**Authentication:** None required

```javascript
const plans = await client.billing().getPlans();
```

```rust
let plans = client.billing().get_plans().await?;
```

**Returns:** `Plan[]`

---

### getPortal / get_portal

Get the billing portal URL for managing subscriptions via Stripe.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const { url } = await client.billing().getPortal('user123');
window.location.href = url; // Redirect to Stripe portal
```

```rust
let response = client.billing().get_portal("user123").await?;
```

**Returns:** `BillingPortalResponse`

| Field | Type | Description |
|-------|------|-------------|
| `url` | `string?` | Stripe billing portal URL |

---

### getSubscription / get_subscription

Get subscription information for a user.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const subscription = await client.billing().getSubscription('user123');
```

```rust
let subscription = client.billing().get_subscription("user123").await?;
```

**Returns:** `Subscription`

---

### createCheckout / create_checkout

Create a checkout session for a new subscription.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `request` | `CheckoutRequest` | Checkout options |

```javascript
const { url } = await client.billing().createCheckout('user123', {
  priceId: 'price_abc123',
  successUrl: 'https://app.example.com/success',
  cancelUrl: 'https://app.example.com/cancel'
});
window.location.href = url; // Redirect to Stripe checkout
```

```rust
let response = client.billing().create_checkout("user123", &request).await?;
```

**Returns:** `CheckoutResponse`

| Field | Type | Description |
|-------|------|-------------|
| `url` | `string?` | Stripe checkout URL |
| `sessionId` | `string?` | Stripe session ID |

**CheckoutRequest Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `priceId` | `string?` | Price ID from the plan |
| `successUrl` | `string?` | Success redirect URL |
| `cancelUrl` | `string?` | Cancel redirect URL |
| `coupon` | `string?` | Coupon code to apply |

---

### modifySubscription / modify_subscription

Modify an existing subscription (upgrade/downgrade).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `request` | `ModifySubscriptionRequest` | Modification options |

```javascript
const subscription = await client.billing().modifySubscription('user123', {
  priceId: 'price_xyz789',
  prorate: true
});
```

```rust
let subscription = client.billing().modify_subscription("user123", &request).await?;
```

**Returns:** `Subscription`

**ModifySubscriptionRequest Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `priceId` | `string?` | New price ID |
| `prorate` | `boolean?` | Whether to prorate |

---

### cancelSubscription / cancel_subscription

Cancel a subscription.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const subscription = await client.billing().cancelSubscription('user123');
// Subscription will remain active until end of current period
```

```rust
let subscription = client.billing().cancel_subscription("user123").await?;
```

**Returns:** `Subscription`

---

### reactivateSubscription / reactivate_subscription

Reactivate a canceled subscription (before it expires).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |

```javascript
const subscription = await client.billing().reactivateSubscription('user123');
```

```rust
let subscription = client.billing().reactivate_subscription("user123").await?;
```

**Returns:** `Subscription`

---

### redeemCoupon / redeem_coupon

Redeem a coupon code.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `request` | `CouponRequest` | Coupon details |

```javascript
const result = await client.billing().redeemCoupon('user123', {
  code: 'SAVE20'
});
if (result.valid) {
  console.log(`Discount: ${result.discount}%`);
}
```

```rust
let result = client.billing().redeem_coupon("user123", &request).await?;
```

**Returns:** `CouponResponse`

| Field | Type | Description |
|-------|------|-------------|
| `valid` | `boolean?` | Whether coupon is valid |
| `discount` | `number?` | Discount amount |
| `discountType` | `string?` | Type (percent_off, amount_off) |
| `error` | `string?` | Error message if invalid |

---

### getEntitlements / get_entitlements

Get available entitlements/features.

**Authentication:** None required

```javascript
const entitlements = await client.billing().getEntitlements();
```

```rust
let entitlements = client.billing().get_entitlements().await?;
```

**Returns:** `Entitlement[]`

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string?` | Entitlement ID |
| `name` | `string?` | Display name |
| `description` | `string?` | Description |
| `enabled` | `boolean?` | Whether enabled |

---

### applyPromo / apply_promo

Apply a promotional code.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `request` | `PromoRequest` | Promo details |

```javascript
const result = await client.billing().applyPromo('user123', {
  code: 'FREETRIAL'
});
```

```rust
let result = client.billing().apply_promo("user123", &request).await?;
```

**Returns:** `PromoResponse`

| Field | Type | Description |
|-------|------|-------------|
| `applied` | `boolean?` | Whether promo was applied |
| `description` | `string?` | Promo description |
| `error` | `string?` | Error if not applied |

---

### listInvoices / list_invoices

List all invoices for a user.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `cursor` | `string?` | Pagination cursor |
| `limit` | `number?` | Items per page |

```javascript
const result = await client.billing().listInvoices('user123', null, 10);
console.log(result.invoices);
if (result.hasMore) {
  const next = await client.billing().listInvoices('user123', result.cursor, 10);
}
```

```rust
let result = client.billing().list_invoices("user123", None, Some(10)).await?;
```

**Returns:** `InvoiceListResponse`

| Field | Type | Description |
|-------|------|-------------|
| `invoices` | `Invoice[]?` | List of invoices |
| `cursor` | `string?` | Next page cursor |
| `hasMore` | `boolean?` | More results available |

**Invoice Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string?` | Invoice ID |
| `number` | `string?` | Invoice number |
| `amount` | `number?` | Amount in cents |
| `currency` | `string?` | Currency code |
| `status` | `string?` | Invoice status |
| `date` | `string?` | Invoice date (RFC3339) |
| `pdfUrl` | `string?` | PDF download URL |
| `hostedUrl` | `string?` | Hosted invoice URL |

