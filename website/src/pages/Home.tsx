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
    </>
  );
}
