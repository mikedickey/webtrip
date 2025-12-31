# Devices API

JackTrip hardware device management and configuration.

## Access

```javascript
const devicesApi = client.devices();
```

```rust
let devices_api = client.devices();
```

## Methods

### listDevices / list_devices

List all devices in the account.

**Authentication:** Required

```javascript
const devices = await client.devices().listDevices();
```

```rust
let devices = client.devices().list_devices().await?;
```

**Returns:** `Device[]`

---

### registerDevice / register_device

Register a new device.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `device` | `Device` | Device configuration |

```javascript
const device = await client.devices().registerDevice({
  name: 'My JackTrip',
  mac: '00:11:22:33:44:55'
});
```

```rust
let device = client.devices().register_device(&new_device).await?;
```

**Returns:** `Device`

---

### getDevice / get_device

Get a device by ID.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `deviceId` | `string` | Device ID |

```javascript
const device = await client.devices().getDevice('device123');
```

```rust
let device = client.devices().get_device("device123").await?;
```

**Returns:** `Device`

---

### updateDevice / update_device

Update a device's configuration.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `deviceId` | `string` | Device ID |
| `device` | `Device` | Updated configuration |

```javascript
const updated = await client.devices().updateDevice('device123', {
  name: 'Updated Name'
});
```

```rust
let updated = client.devices().update_device("device123", &device).await?;
```

**Returns:** `Device`

---

### deleteDevice / delete_device

Delete a device.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `deviceId` | `string` | Device ID |

```javascript
await client.devices().deleteDevice('device123');
```

```rust
client.devices().delete_device("device123").await?;
```

**Returns:** `void` / `()`

---

### sendHeartbeat / send_heartbeat

Send a device heartbeat. Used by JackTrip devices to report status.

**Authentication:** Required (device auth)

| Parameter | Type | Description |
|-----------|------|-------------|
| `deviceId` | `string` | Device ID |
| `heartbeat` | `HeartbeatRequest` | Heartbeat data |

```javascript
const config = await client.devices().sendHeartbeat('device123', {
  version: '1.0.0',
  mac: '00:11:22:33:44:55',
  pktsRecv: 1000,
  pktsSent: 1000,
  avgRtt: 15
});
```

```rust
let config = client.devices().send_heartbeat("device123", &heartbeat).await?;
```

**Returns:** `DeviceAgentConfig`

**HeartbeatRequest Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `apiPrefix` | `string?` | Device API key prefix |
| `apiSecret` | `string?` | Device API key secret |
| `mac` | `string?` | Device MAC address |
| `version` | `string?` | Device version |
| `type` | `string?` | ALSA device type |
| `pktsRecv` | `number?` | Packets received |
| `pktsSent` | `number?` | Packets sent |
| `minRtt` | `number?` | Minimum RTT (ms) |
| `maxRtt` | `number?` | Maximum RTT (ms) |
| `avgRtt` | `number?` | Average RTT (ms) |
| `stddevRtt` | `number?` | RTT standard deviation |
| `latestRtt` | `number?` | Latest RTT (ms) |
| `statsUpdatedAt` | `string?` | Stats timestamp (RFC3339) |

---

### listStudioDevices / list_studio_devices

List devices connected to a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |

```javascript
const devices = await client.devices().listStudioDevices('studio123');
```

```rust
let devices = client.devices().list_studio_devices("studio123").await?;
```

**Returns:** `Device[]`

---

### updateCaptureVolume / update_capture_volume

Update capture volume for all devices in a studio.

**Authentication:** Required

| Parameter | Type | Description |
|-----------|------|-------------|
| `studioId` | `string` | Studio ID |
| `min` | `number?` | Minimum volume |
| `max` | `number?` | Maximum volume |

```javascript
await client.devices().updateCaptureVolume('studio123', 0, 100);
```

```rust
client.devices().update_capture_volume("studio123", Some(0), Some(100)).await?;
```

**Returns:** `void` / `()`

