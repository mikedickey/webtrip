const GITHUB_DOCS_URL = "https://github.com/mikedickey/webtrip/tree/main/docs";

export default function Docs() {
  return (
    <section className="page">
      <h1>Documentation</h1>
      <p className="lede">
        Full documentation for WebTrip is under construction and will live
        here.
      </p>
      <p>
        In the meantime, the{" "}
        <a href={GITHUB_DOCS_URL} target="_blank" rel="noreferrer">
          docs directory on GitHub
        </a>{" "}
        covers the architecture, threading model, and testing setup.
      </p>
    </section>
  );
}
