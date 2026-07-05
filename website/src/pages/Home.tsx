import { Link } from "react-router-dom";

const FEATURES = [
  {
    title: "Zero install",
    body: "Runs entirely in any modern browser on any popular device. Nothing to download or buy. No apps, no drivers, no setup — just open a link and connect.",
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

const USE_CASES = [
  {
    title: "Natural conversations",
    body: "Tone, inflection, pauses — the cues that carry meaning beyond words — are exactly what lossy compression flattens. And when the round trip is short, turn-taking just works: no talking over each other, no awkward gaps.",
  },
  {
    title: "AI voice",
    body: "Cleaner audio in means more accurate speech-to-text out — fewer misheard words, fewer wrong answers. Lower latency cuts the wait for a response, so talking to an agent feels like a conversation instead of a walkie-talkie.",
  },
  {
    title: "Music lessons",
    body: "A teacher has to hear exactly what the student played — the full spectrum, every dynamic and overtone. Codecs tuned for speech discard the detail that useful feedback depends on.",
  },
  {
    title: "Writing songs together",
    body: "Trading riffs, finding a harmony, feeling out a groove — collaboration this immediate falls apart with even modest delay. And a rough idea's character lives in nuances that speech codecs strip away.",
  },
  {
    title: "Therapy & guided meditation",
    body: "These sessions work because of how a voice sounds: calm, warm, present. Compression artifacts and dropouts break the connection that the session is trying to build.",
  },
  {
    title: "Prayer & chanting",
    body: "Voices joined in chant or prayer have to stay together. Degraded audio disrupts the flow, and high latency makes a shared rhythm impossible.",
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

function CardGrid({
  items,
  heading: Heading = "h3",
}: {
  items: { title: string; body: string }[];
  heading?: "h2" | "h3";
}) {
  return (
    <div className="card-grid">
      {items.map((item) => (
        <article className="card" key={item.title}>
          <Heading>{item.title}</Heading>
          <p>{item.body}</p>
        </article>
      ))}
    </div>
  );
}

const BENCH_SYSTEMS = [
  { name: "Zoom", quality: 192, latency: 155 },
  { name: "Google Meet", quality: 128, latency: 160 },
  { name: "WebRTC", quality: 64, latency: 160 },
  { name: "WebTrip", quality: 1536, latency: 35, isWebTrip: true },
];

function BenchPanel({
  title,
  hint,
  metric,
  format,
}: {
  title: string;
  hint: string;
  metric: "quality" | "latency";
  format: (value: number) => string;
}) {
  const max = Math.max(...BENCH_SYSTEMS.map((s) => s[metric]));
  return (
    <article className="card bench-panel">
      <h3>
        {title}
        <span className="bench-hint">{hint}</span>
      </h3>
      {BENCH_SYSTEMS.map((s) => (
        <div
          className={s.isWebTrip ? "bench-row bench-row-webtrip" : "bench-row"}
          key={s.name}
        >
          <div className="bench-row-head">
            <span className="bench-name">{s.name}</span>
            <span className="bench-value">{format(s[metric])}</span>
          </div>
          <div className="bench-track">
            <div
              className="bench-bar"
              style={{ width: `${(s[metric] / max) * 100}%` }}
            />
          </div>
        </div>
      ))}
    </article>
  );
}

export default function Home() {
  return (
    <>
      <section className="hero">
        <h1>
          Lossless, low&#8209;latency audio collaboration.
          <br />
          <span className="hero-accent">In your web browser.</span>
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
        <CardGrid items={FEATURES} heading="h2" />
      </section>

      <section className="use-cases">
        <h2 className="section-title">More than words</h2>
        <p className="section-intro">
          The human voice carries far more information than the words alone —
          emotion, tone, inflection, timing. High-fidelity audio preserves
          those cues, and low latency keeps the exchange natural. Both are
          essential wherever people (or machines) really listen.
        </p>
        <CardGrid items={USE_CASES} />
      </section>

      <section className="comparison">
        <h2 className="section-title">Why not just use WebRTC?</h2>
        <p className="section-intro">
          Browsers already ship real-time audio through WebRTC&rsquo;s media
          stack — but it&rsquo;s optimized for speech, compressed and tuned to
          keep voices intelligible rather than faithful. WebTrip takes a
          different path: raw, uncompressed audio, skipping the
          browser&rsquo;s built-in pipeline for higher fidelity and lower
          latency.
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
                  <td className="col-webtrip" data-label="WebTrip">
                    {row.webtrip}
                  </td>
                  <td data-label="WebRTC">{row.webrtc}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="card-grid bench-grid">
          <BenchPanel
            title="Audio quality"
            hint="higher is better"
            metric="quality"
            format={(v) => `${v.toLocaleString("en-US")} kbps`}
          />
          <BenchPanel
            title="Audio latency"
            hint="lower is better"
            metric="latency"
            format={(v) => `${v} ms`}
          />
        </div>
        <p className="bench-note">
          Typical figures; actual performance varies with hardware and network
          conditions.
        </p>
      </section>
    </>
  );
}
