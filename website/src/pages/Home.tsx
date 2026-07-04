import { Link } from "react-router-dom";

const FEATURES = [
  {
    title: "Zero install",
    body: "Runs entirely in any modern browser on any popular device. No apps, no drivers, no setup — just open a link and play.",
  },
  {
    title: "Built for low latency",
    body: "WebTransport (QUIC datagrams) with WebRTC fallback, an AudioWorklet processing pipeline, and lock-free buffers between threads.",
  },
  {
    title: "Rust at the core",
    body: "The audio engine is written in Rust and compiled to WebAssembly with threads and shared memory, designed for reuse by developers.",
  },
  {
    title: "Lossless audio",
    body: "Uncompressed audio over the JackTrip wire protocol, with a jitter buffer and Burg packet-loss concealment ported from JackTrip.",
  },
];

const COMPARISON = [
  {
    dimension: "Fidelity",
    webtrip: "Uncompressed PCM — lossless.",
    webrtc: "Opus compression — lossy by design.",
  },
  {
    dimension: "Latency",
    webtrip: "2.7 ms audio chunks, zero codec delay.",
    webrtc: "20 ms frames plus look-ahead — ~26 ms of codec delay built in.",
  },
  {
    dimension: "Transport",
    webtrip: "WebTransport — QUIC datagrams, no head-of-line blocking.",
    webrtc: "SRTP, negotiated over SDP and ICE.",
  },
  {
    dimension: "Loss & Jitter",
    webtrip:
      "Low-latency jitter buffer with Burg packet-loss concealment.",
    webrtc: "Fixed adaptive buffer and codec concealment.",
  },
  {
    dimension: "Processing",
    webtrip:
      "Optimized Rust/WASM engine in an AudioWorklet; network I/O in a dedicated worker.",
    webrtc: "Opaque browser pipeline; varies by browser, can't be tuned.",
  },
];

export default function Home() {
  return (
    <>
      <section className="hero">
        <h1>
          Lossless, low&#8209;latency audio collaboration.
          <br />
          <span className="hero-accent">In your browser.</span>
        </h1>
        <p className="lede">
          WebTrip is a software development toolkit for real-time audio over
          the Internet — a rewrite of JackTrip&rsquo;s core in Rust and
          WebAssembly that runs without installing anything.
        </p>
        <div className="cta-row">
          <Link className="btn btn-primary" to="/demo">
            Try the demo
          </Link>
          <Link className="btn btn-secondary" to="/docs">
            Read the docs
          </Link>
        </div>
      </section>

      <section className="features">
        {FEATURES.map((f) => (
          <article className="feature-card" key={f.title}>
            <h2>{f.title}</h2>
            <p>{f.body}</p>
          </article>
        ))}
      </section>

      <section className="comparison">
        <h2>Why not just use WebRTC?</h2>
        <p className="comparison-intro">
          Browsers already ship real-time audio through WebRTC&rsquo;s media
          stack. WebTrip takes a different path: raw, uncompressed audio,
          skipping the browser&rsquo;s built-in pipeline for higher fidelity
          and lower latency.
        </p>
        <div className="comparison-scroll">
          <table className="comparison-table">
            <thead>
              <tr>
                <th scope="col">
                  <span className="visually-hidden">Dimension</span>
                </th>
                <th scope="col" className="col-webtrip">
                  WebTrip
                </th>
                <th scope="col">WebRTC</th>
              </tr>
            </thead>
            <tbody>
              {COMPARISON.map((row) => (
                <tr key={row.dimension}>
                  <th scope="row">{row.dimension}</th>
                  <td className="col-webtrip">{row.webtrip}</td>
                  <td>{row.webrtc}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </>
  );
}
