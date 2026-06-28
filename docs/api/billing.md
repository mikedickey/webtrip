# Billing API

Subscription and payment management.

## Access

```javascript
const billingApi = client.billing();
```

```rust
let billing_api = client.billing();
```

Plan changes (upgrade/downgrade/cancel/reactivate) are handled through the
Stripe-hosted billing portal, so this client only exposes plan-price lookup plus
the portal/checkout redirect surfaces.

## Methods

### getPlans / get_plans

Resolve the Stripe price for a plan / pricing mode (`GET /users/{userId}/plans`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `plan` | `string` | Plan name to resolve (required) |
| `pricingMode` | `string?` | Pricing mode (e.g. `yearly`) |
| `forceStripeTestMode` | `string?` | Force Stripe test mode |

```javascript
const { plan, priceID } = await client.billing().getPlans('user123', 'pro', 'yearly', null);
```

```rust
let resolved = client.billing().get_plans("user123", "pro", Some("yearly"), None).await?;
```

**Returns:** `PlanPrice`

| Field | Type | Description |
|-------|------|-------------|
| `plan` | `string?` | Resolved plan name |
| `priceID` | `string?` | Stripe price ID for the requested plan and pricing mode |

---

### getPortal / get_portal

Create a Stripe billing-portal session and return its redirect URL
(`POST /users/{userId}/billing`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `request` | `BillingPortalRequest` | `{ callbackURL }` |

```javascript
const { redirect } = await client.billing().getPortal('user123', {
  callbackURL: 'https://app.example.com/account'
});
window.location.href = redirect; // Redirect to Stripe portal
```

```rust
let request = BillingPortalRequest { callback_url: Some("https://app.example.com/account".into()) };
let response = client.billing().get_portal("user123", &request).await?;
```

**Returns:** `Redirect`

| Field | Type | Description |
|-------|------|-------------|
| `redirect` | `string?` | Stripe billing portal URL |

---

### createCheckout / create_checkout

Create a Stripe checkout session and return its redirect URL
(`POST /users/{userId}/checkout`).

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `userId` | `string` | User ID |
| `request` | `CheckoutRequest` | Checkout options |

```javascript
const { redirect } = await client.billing().createCheckout('user123', {
  plan: 'pro',
  pricingMode: 'yearly',
  callbackURL: 'https://app.example.com/done'
});
window.location.href = redirect; // Redirect to Stripe checkout
```

```rust
let request = CheckoutRequest {
    plan: "pro".into(),
    callback_url: "https://app.example.com/done".into(),
    ..Default::default()
};
let response = client.billing().create_checkout("user123", &request).await?;
```

**Returns:** `Redirect`

| Field | Type | Description |
|-------|------|-------------|
| `redirect` | `string?` | Stripe-hosted checkout URL |

**CheckoutRequest Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `plan` | `string` | Subscription plan name to check out (required) |
| `callbackURL` | `string` | URL to redirect to after checkout completes (required) |
| `pricingMode` | `string?` | Pricing mode (e.g. `yearly`) |
| `forceStripeTestMode` | `boolean?` | Force Stripe test mode regardless of environment |

