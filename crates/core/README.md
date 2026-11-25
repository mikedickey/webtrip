# webtrip-core

The portable core of [WebTrip](https://webtrip.dev): audio processing, the JackTrip
wire protocol, the jitter buffer, and the transport contract that the platform
hosts implement.

Nothing in this crate depends on a browser. It builds natively and on
`wasm32-unknown-unknown`, and its shared state is safe to use across real threads.
In the browser build, the main thread, the AudioWorklet render thread and the
WebTransport worker all reach it over one shared WASM memory.

## Modules

- [`network`] — the JackTrip 16-byte wire protocol ([`network::protocol`]) and the
  [`Transport`](network::Transport) trait that platform transports implement
- [`stream`] — the bridge between network and audio: the
  [`Regulator`](stream::regulator::Regulator) jitter buffer with Burg packet-loss
  concealment, the lock-free SPSC [`RingBuffer`](stream::ring_buffer::RingBuffer),
  and [`SharedPtr`](stream::shared_ptr::SharedPtr) for cross-thread pointers
- [`audio`] — channel mixing, resampling, and the atomic
  [`AudioParams`](audio::params::AudioParams) shared between threads
- [`limits`] — channel, rate and packet-size bounds shared by every layer

`network` and `audio` never import each other; both use `stream`. The host composes
them.

## Example

```rust
use webtrip_core::network::protocol::AudioPacket;

// 64 frames of interleaved stereo silence: sequence 7, timestamp 0.
let packet = AudioPacket::stereo(7, 0, vec![0.0; 64 * 2]);
let bytes = packet.serialize()?;

let decoded = AudioPacket::deserialize(&bytes)?;
assert_eq!(decoded.header.sequence_number, 7);
assert_eq!(decoded.header.buffer_size, 64);
# Ok::<(), webtrip_core::network::protocol::ProtocolError>(())
```

## See also

- [`webtrip`](https://docs.rs/webtrip): the one-dependency facade over all WebTrip crates
- [Architecture](https://webtrip.dev/docs/architecture): threads, data flow and design rationale
