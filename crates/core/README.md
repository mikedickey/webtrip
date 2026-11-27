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
- [`session_stats`] — the [`SessionStats`](session_stats::SessionStats) snapshot a
  host reports for its connections

`network` and `audio` never import each other; both use `stream`. The host composes
them.

## How the pieces fit

A host (the browser host in `webtrip-web`, or your own) owns the threads, the audio
device and the sockets, and wires one connection like this:

```text
send:     audio callback ─▶ AudioProcessor::process ─▶ RingBuffer ─▶ AudioPacket ─▶ Transport
receive:  Transport ─▶ AudioPacket::deserialize_into ─▶ Regulator::push_at
          audio callback ─▶ AudioProcessor::process ─▶ Regulator::pop_frames_at ─▶ speakers
```

- On the **audio thread**, [`AudioProcessor`](audio::AudioProcessor) runs once per
  device callback. It writes the capture into the connection's
  [`RingBuffer`](stream::ring_buffer::RingBuffer) and pops peer audio from its
  [`Regulator`](stream::regulator::Regulator), converting between the device rate
  and the 48 kHz wire rate when they differ.
- On the **send path**, a thread waits for one packet's worth of samples in the ring,
  reads them, and serializes them with
  [`AudioPacket::serialize_samples_into`](network::protocol::AudioPacket::serialize_samples_into).
- On the **receive path**, each datagram is decoded and pushed into the regulator,
  which reorders, buffers and conceals lost packets.
- [`AudioParams`](audio::AudioParams) carries gain, volume and meters between the
  control thread and the audio thread.

The ring and the regulator are single-producer, single-consumer: one thread on each
side. A [`Transport`](network::Transport) implementation owns the send and receive
paths for one connection.

## Examples

Encode and decode a packet:

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

Receive datagrams into a regulator and play them out (in a real host the two halves
run on different threads, sharing the regulator through an `Arc`):

```rust
use webtrip_core::limits::{MAX_WIRE_CHANNELS, WIRE_SAMPLE_RATE};
use webtrip_core::network::protocol::AudioPacket;
use webtrip_core::stream::regulator::{PushOutcome, Regulator};

let regulator = Regulator::with_params(2, 64, WIRE_SAMPLE_RATE, -1.0);
let datagram = AudioPacket::stereo(0, 0, vec![0.25; 64 * 2]).serialize()?;

// Network thread: decode, then push with the header's shape and the arrival time.
let mut samples = Vec::new();
let header = AudioPacket::deserialize_into(&datagram, &mut samples)?;
let outcome = regulator.push_at(
    header.sequence_number,
    header.num_outgoing_channels as usize,
    header.buffer_size as usize,
    &samples,
    0.0,
);
assert_eq!(outcome, PushOutcome::Stored);

// Audio thread: one packet-sized callback, on the same millisecond clock.
let mut out = vec![0.0; 64 * MAX_WIRE_CHANNELS as usize];
let channels = regulator.pop_frames_at(&mut out, 64, 1.3);
assert_eq!(channels, 2);
# Ok::<(), webtrip_core::network::protocol::ProtocolError>(())
```

## See also

- [`webtrip`](https://docs.rs/webtrip): the one-dependency facade over all WebTrip crates
- [Architecture](https://webtrip.dev/docs/architecture): threads, data flow and design rationale
