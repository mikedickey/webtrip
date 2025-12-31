# JackTrip WebAssembly Architecture

## Browser API Limitations and Workarounds

This document explains the architectural constraints imposed by browser APIs and how we work around them in the JackTrip WebAssembly implementation.

---

## Table of Contents

1. [Threading Model](#threading-model)
2. [Limitation 1: WebRTC on Main Thread Only](#limitation-1-webrtc-on-main-thread-only)
3. [Limitation 2: AudioWorklet Thread Isolation](#limitation-2-audioworklet-thread-isolation)
4. [Limitation 3: No Direct Thread Communication](#limitation-3-no-direct-thread-communication)
5. [Workaround: Global Static Buffers](#workaround-global-static-buffers)
6. [Workaround: Main Thread Tick Loop](#workaround-main-thread-tick-loop)
7. [Complete Data Flow](#complete-data-flow)
8. [Trade-offs and Alternatives](#trade-offs-and-alternatives)

---

## Threading Model

The browser provides multiple execution contexts with strict boundaries:

```
┌─────────────────────────────────────────────────────────────┐
│                    Main Browser Thread                       │
│  - All WebRTC APIs (RTCPeerConnection, RTCDataChannel)      │
│  - DOM manipulation                                          │
│  - WebSocket, fetch, all network APIs                       │
│  - JavaScript execution                                      │
│  - WebAssembly module instantiation                          │
└─────────────────────────────────────────────────────────────┘
                            │
                            │ MessagePort (limited)
                            │
┌─────────────────────────────────────────────────────────────┐
│                   AudioWorklet Thread                        │
│  - Real-time audio processing (process() callback)          │
│  - Runs at audio sample rate (~128 samples every 2.67ms)   │
│  - CANNOT access: WebRTC, DOM, most browser APIs            │
│  - CAN access: SharedArrayBuffer, atomics, AudioParams      │
└─────────────────────────────────────────────────────────────┘

        Both threads share WebAssembly linear memory
```

---

## Limitation 1: WebRTC on Main Thread Only

### The Problem

**WebRTC APIs are only available on the main browser thread.**

```javascript
// ✅ Main thread - works
const dataChannel = peerConnection.createDataChannel("audio");
dataChannel.send(audioData);

// ❌ AudioWorklet thread - DOES NOT WORK
// RTCDataChannel is undefined in worklet scope
class MyProcessor extends AudioWorkletProcessor {
  process(inputs, outputs) {
    // Can't call dataChannel.send() here!
    // Can't access any WebRTC APIs!
  }
}
```

### Why This Matters

For real-time audio streaming, we need to:
- **Capture audio** in the AudioWorklet (low latency)
- **Send audio** over WebRTC data channel (main thread only)
- **Receive audio** from WebRTC data channel (main thread only)
- **Play audio** in the AudioWorklet (low latency)

The audio processing and network I/O happen in **different threads**.

---

## Limitation 2: AudioWorklet Thread Isolation

### The Problem

**The AudioWorklet thread cannot access most JavaScript objects.**

What you **CAN** pass to AudioWorklet:
- Numbers (pointers, IDs)
- TypedArrays (Float32Array, Uint8Array, etc.)
- SharedArrayBuffer
- AudioParams
- Serializable objects via MessagePort

What you **CANNOT** pass:
- WebRTC objects (RTCDataChannel, RTCPeerConnection)
- DOM elements
- Functions/closures
- Arbitrary Rust structs
- Most JavaScript objects

### Example

```rust
// ❌ Cannot do this
struct AudioProcessor {
    data_channel: RTCDataChannel,  // Can't pass WebRTC object to worklet!
}

// ✅ Must do this instead
struct AudioProcessor {
    buffer_ptr: *mut RingBuffer,  // Just a number (memory address)
}
```

---

## Limitation 3: No Direct Thread Communication

### The Problem

**AudioWorklet and main thread cannot directly call each other's functions.**

Communication options are extremely limited:
1. **MessagePort**: Async message passing (too slow for real-time audio)
2. **SharedArrayBuffer + Atomics**: Shared memory (manual synchronization)
3. **AudioParams**: Float values only (very limited)

### Why MessagePort Doesn't Work

```javascript
// Main thread
workletNode.port.postMessage({ audio: samples });

// AudioWorklet receives messages ASYNCHRONOUSLY
class MyProcessor extends AudioWorkletProcessor {
  constructor() {
    this.port.onmessage = (e) => {
      // ❌ This callback doesn't run during process()!
      // ❌ It runs between process() calls - unpredictable timing!
    };
  }
  
  process() {
    // Need audio data NOW, can't wait for async message
  }
}
```

For real-time audio (~2.7ms intervals at 128 samples/48kHz), we need **synchronous, deterministic access** to shared data.

---

## Workaround: Session-Owned Buffers with Shared Pointers

### The Solution

**Buffers are owned by JackTripSession** (main thread) but **accessed by both threads via raw pointers**.

The key insight: WebAssembly linear memory is **shared across all threads** in the same module instance. We don't need global statics - we just need to ensure the buffers stay alive while being accessed.

### Implementation

```rust
// session.rs
pub struct JackTripSession {
    // Session owns the buffers (allocated in WebAssembly linear memory)
    local_to_network_buffer: Box<RingBuffer>,
    network_to_local_buffer: Box<LockFreeJitterBuffer>,
    
    audio_engine: Option<AudioEngine>,
    // ...
}

impl JackTripSession {
    pub fn new() -> Self {
        Self {
            // Buffers are created here, owned by Session
            local_to_network_buffer: Box::new(RingBuffer::new()),
            network_to_local_buffer: Box::new(LockFreeJitterBuffer::new()),
            audio_engine: None,
        }
    }
    
    pub async fn start_capture(&mut self) -> Result<(), JsValue> {
        // Get raw pointers to owned buffers
        let local_to_network_ptr = &mut *self.local_to_network_buffer as *mut RingBuffer;
        let network_to_local_ptr = &*self.network_to_local_buffer as *const LockFreeJitterBuffer;
        
        // Pass pointers to AudioEngine (and eventually to AudioWorklet)
        let engine = AudioEngine::create_with_network(
            self.audio_params_ptr,
            local_to_network_ptr,
            network_to_local_ptr,
        ).await?;
        
        self.audio_engine = Some(engine);
        Ok(())
    }
}
```

### How It Works

1. **Session creates and owns** the buffers in WebAssembly linear memory:
   ```rust
   let session = JackTripSession::new();
   // Buffers are at addresses 0x12345678 and 0x23456789 (examples)
   ```

2. **Get raw pointers** (just memory addresses, not Rust object references):
   ```rust
   let ptr = &mut *self.local_to_network_buffer as *mut RingBuffer;
   // ptr = 0x12345678
   ```

3. **Pass pointers** to AudioWorklet (pointers are just numbers):
   ```rust
   AudioEngine::create_with_network(params_ptr, local_to_network_ptr, network_to_local_ptr)
   ```

4. **Both threads access the same memory**:
   ```rust
   // Main thread (Session)
   self.local_to_network_buffer.read(&mut audio)
   
   // AudioWorklet thread (via pointer)
   unsafe { (*ptr).write(samples) }
   ```

### Memory Layout

```
WebAssembly Linear Memory (shared between all threads)
┌─────────────────────────────────────────────────┐
│  Address 0x1000: RingBuffer                     │
│    - buffer: [f32; 4096]                        │
│    - write_pos: AtomicU32                       │
│    - read_pos: AtomicU32                        │
│    - ...                                        │
│                                                  │
│  Address 0x5000: JitterBuffer                   │
│    - slots: [PacketSlot; 64]                    │
│    - read_sequence: AtomicU64                   │
│    - ...                                        │
└─────────────────────────────────────────────────┘
         ↑                              ↑
         │                              │
    Main Thread                    AudioWorklet
    (via pointer)                  (via same pointer)
```

### Thread Safety

We use **atomics** for synchronization:

```rust
pub struct RingBuffer {
    buffer: Vec<f32>,
    write_pos: AtomicU32,  // Lock-free atomic operations
    read_pos: AtomicU32,
    // ...
}

// Worklet writes
impl RingBuffer {
    pub fn write(&mut self, samples: &[f32]) -> bool {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = self.read_pos.load(Ordering::Acquire);
        // ... write data ...
        self.write_pos.store(new_pos, Ordering::Release);
    }
}

// Main thread reads
impl RingBuffer {
    pub fn read(&mut self, output: &mut [f32]) -> bool {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        // ... read data ...
        self.read_pos.store(new_pos, Ordering::Release);
    }
}
```

### Lifetime Management

**Critical**: The buffers must outlive any pointer usage. We ensure this with careful Drop ordering:

```rust
impl Drop for JackTripSession {
    fn drop(&mut self) {
        // MUST stop AudioWorklet BEFORE dropping buffers
        self.stop_capture();  // Stops worklet, no more pointer access
        // Now it's safe to drop buffers
    }
}

impl JackTripSession {
    pub fn stop_capture(&mut self) {
        // Stop AudioEngine (disconnects AudioWorklet)
        if let Some(ref mut engine) = self.audio_engine {
            engine.stop_capture();
        }
        self.audio_engine = None;
        // AudioWorklet is now stopped, won't access buffer pointers anymore
        // Buffers remain valid (owned by Session) but no longer accessed
    }
}
```

### Why Not Global Statics?

**We considered global statics** (like `static mut RING_BUFFER: Option<RingBuffer>`) but that approach has significant downsides:

1. ❌ **Global mutable state** - Goes against Rust idioms
2. ❌ **Only one session** - Can't have multiple instances
3. ❌ **Unclear ownership** - Who's responsible for cleanup?
4. ❌ **Unsafe throughout** - More `unsafe` blocks everywhere

**Session-owned buffers are better:**

✅ **Clear ownership** - Session owns buffers, controls lifetime  
✅ **Multiple sessions** - Could create multiple JackTripSession instances  
✅ **Safer** - Only `unsafe` at the boundary where pointers are created/used  
✅ **Standard Rust patterns** - Follows normal RAII and Drop semantics

---

## Workaround: Main Thread Tick Loop

### The Problem

Since WebRTC APIs are only available on the main thread, we need a **polling loop** to bridge the gap between audio and network.

### The Solution

Run a high-frequency interval on the main thread that:
1. Reads audio from the RingBuffer (written by AudioWorklet)
2. Sends audio via WebRTC data channel
3. Receives audio from WebRTC data channel
4. Writes audio to JitterBuffer (read by AudioWorklet)

### Implementation

```rust
// session.rs
fn start_network_loop(&mut self) {
    let window = web_sys::window().expect("no global window");
    
    let session_ptr = self as *mut JackTripSession;
    let closure = Closure::wrap(Box::new(move || {
        unsafe { (*session_ptr).tick(); }
    }) as Box<dyn FnMut()>);
    
    // Run every 5ms (200 times per second)
    let handle = window.set_interval_with_callback_and_timeout_and_arguments_0(
        closure.as_ref().unchecked_ref(),
        5,
    ).expect("failed to set interval");
    
    closure.forget();
    self.interval_handle = Some(handle);
}

pub fn tick(&mut self) {
    // === SEND PATH: local audio → network ===
    if !self.local_to_network_buffer_ptr.is_null() {
        let buffer = unsafe { &mut *self.local_to_network_buffer_ptr };
        if buffer.available() >= self.buffer_size as u32 {
            if buffer.read(&mut self.audio_to_send_buffer) {
                self.client.send_audio(&self.audio_to_send_buffer)?;
            }
        }
    }

    // === RECEIVE PATH: network → local audio ===
    if let Ok(samples) = self.client.receive_audio() {
        if !samples.is_empty() {
            let buffer = unsafe { &*self.network_to_local_buffer_ptr };
            buffer.push(self.sequence_number, &samples);
            self.sequence_number += 1;
        }
    }
}
```

### Why 5ms Interval?

- **Audio buffer size**: 128 samples at 48kHz = ~2.67ms per process() call
- **Tick rate**: 5ms = 200Hz
- **Trade-off**: Fast enough to handle audio in real-time, but not so fast that it overwhelms the main thread

### Why Can't AudioWorklet Call tick()?

```rust
// ❌ Would be nice, but impossible
impl AudioProcessor {
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> bool {
        // Write local audio
        self.send_local_to_network();
        
        // ❌ Can't call WebRTC from here!
        webrtc_data_channel.send(audio);  // RTCDataChannel doesn't exist in worklet!
        
        // Read remote audio
        self.receive_from_network();
        
        true
    }
}
```

The tick() loop is the **only way** to access WebRTC APIs while the AudioWorklet handles real-time processing.

---

## Complete Data Flow

### Send Path: Microphone → Network

```
Audio Device                AudioWorklet Thread              Main Thread
    │                              │                             │
    │ ① PCM samples                │                             │
    ├──────────────────────────────>                             │
    │                              │                             │
    │                        ② Apply gain                        │
    │                        ③ Volume metering                   │
    │                              │                             │
    │                        ④ Write to RingBuffer               │
    │                        ring_buffer.write()                 │
    │                              │                             │
    │                              │    (shared memory)          │
    │                              │ ────────────────────────>   │
    │                              │                             │
    │                              │                      ⑤ tick() reads
    │                              │                   ring_buffer.read()
    │                              │                             │
    │                              │                   ⑥ Send via WebRTC
    │                              │                   dataChannel.send()
    │                              │                             │
    │                              │                             ├─> Internet
```

### Receive Path: Network → Speakers

```
Internet                    Main Thread              AudioWorklet Thread        Audio Device
    │                              │                             │                    │
    ├────> ⑦ Receive packet        │                             │                    │
    │   dataChannel.onmessage      │                             │                    │
    │                              │                             │                    │
    │               ⑧ Push to JitterBuffer                       │                    │
    │            jitter_buffer.push()                            │                    │
    │                              │                             │                    │
    │                              │      (shared memory)        │                    │
    │                              │ <───────────────────────────│                    │
    │                              │                             │                    │
    │                              │                  ⑨ Read from JitterBuffer       │
    │                              │               jitter_buffer.pop()               │
    │                              │                             │                    │
    │                              │                ⑩ Mix with monitor               │
    │                              │                ⑪ Apply output volume            │
    │                              │                             │                    │
    │                              │                             │  ⑫ PCM samples    │
    │                              │                             ├───────────────────>│
```

### Key Points

- **Steps ①-④**: Happen in real-time in AudioWorklet.process() (~2.7ms intervals)
- **Steps ⑤-⑥**: Happen in main thread tick() loop (5ms intervals)
- **Steps ⑦-⑧**: Happen asynchronously when packets arrive (unpredictable timing)
- **Steps ⑨-⑫**: Happen in real-time in AudioWorklet.process()
- **RingBuffer**: Decouples AudioWorklet write rate from network send rate
- **JitterBuffer**: Decouples network receive rate from AudioWorklet read rate

---

## Trade-offs and Alternatives

### Current Approach: Session-Owned Buffers + Tick Loop

**Pros:**
- ✅ Works within browser constraints
- ✅ Type-safe Rust abstractions over shared memory
- ✅ Lock-free atomics for performance
- ✅ Predictable real-time behavior in AudioWorklet
- ✅ Clear ownership and lifetime management
- ✅ Standard Rust RAII patterns
- ✅ Could support multiple sessions

**Cons:**
- ⚠️ Requires careful Drop ordering (AudioWorklet must stop before buffers drop)
- ⚠️ Unsafe raw pointer usage at thread boundaries
- ❌ Main thread polling adds CPU overhead

### Alternative 1: Manual SharedArrayBuffer

Use `SharedArrayBuffer` directly from JavaScript:

**Pros:**
- No Rust global statics

**Cons:**
- Lose Rust type safety
- Manual atomic operations in JavaScript
- More complex, error-prone code
- Still need tick() loop

### Alternative 2: AudioWorkletProcessor MessagePort

Use MessagePort for communication:

**Pros:**
- No shared memory needed
- No global statics

**Cons:**
- ❌ **Asynchronous**: Messages don't arrive during process()
- ❌ **Unpredictable timing**: Can't guarantee real-time behavior
- ❌ **Fundamental limitation**: Still can't call WebRTC from worklet
- Still need tick() loop on main thread

### Alternative 3: Redesigned WebRTC API

What we'd ideally want:

```javascript
// Hypothetical future API (doesn't exist)
class AudioWorkletProcessor {
  process(inputs, outputs) {
    // ❌ This doesn't work and likely never will
    this.dataChannel.send(outputs[0][0]);
  }
}
```

**Reality**: Browser security model keeps threads isolated. WebRTC will likely **never** be available in AudioWorklet.

---

## Conclusion

The current architecture uses:

1. **Session-owned buffers** - Buffers allocated in WebAssembly linear memory, owned by Session, accessed via pointers
2. **Main thread tick loop** - Required because WebRTC APIs only work on the main thread
3. **Lock-free ring buffers** - Efficient, wait-free communication without blocking
4. **Jitter buffer with atomics** - Thread-safe packet reordering and loss concealment
5. **Careful lifetime management** - AudioWorklet must stop before buffers are dropped

These are **not design choices** but **necessary workarounds** for fundamental browser API limitations. Until browsers provide:
- WebRTC access from AudioWorklet, or
- Synchronous cross-thread communication, or  
- AudioWorklet access to network APIs

...we're stuck with this architecture.

The good news: It works well! The use of atomics ensures:
- **Lock-free**: No blocking in the real-time audio thread
- **Wait-free**: AudioWorklet.process() always completes quickly
- **Thread-safe**: No race conditions or data corruption

---

## Related Reading

- [AudioWorklet Design Pattern](https://developers.google.com/web/updates/2017/12/audio-worklet-design-pattern)
- [SharedArrayBuffer and Atomics](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer)
- [WebRTC Data Channels](https://developer.mozilla.org/en-US/docs/Web/API/RTCDataChannel)
- [WebAssembly Threading](https://web.dev/webassembly-threads/)

