import { Link, NavLink, Route, Routes } from "react-router-dom";
import Home from "./pages/Home";
import Docs from "./pages/Docs";
import Demo from "./pages/demo/Demo";

const GITHUB_URL = "https://github.com/mikedickey/webtrip";

export default function App() {
  return (
    <div className="site">
      <header className="site-header">
        <Link to="/" className="brand">
          Web<span className="brand-accent">Trip</span>
        </Link>
        <nav className="site-nav">
          <NavLink to="/" end>
            Home
          </NavLink>
          <NavLink to="/docs">Docs</NavLink>
          <NavLink to="/demo">Demo</NavLink>
          <a
            href={GITHUB_URL}
            target="_blank"
            rel="noreferrer"
            className="github-btn"
            aria-label="WebTrip on GitHub"
          >
            <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true">
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"/>
            </svg>
            GitHub
          </a>
        </nav>
      </header>

      <main className="site-main">
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/docs" element={<Docs />} />
          <Route path="/demo" element={<Demo />} />
          <Route
            path="*"
            element={
              <section className="page">
                <h1>Page not found</h1>
                <p className="lede">
                  There&rsquo;s nothing at this address. Try the{" "}
                  <Link to="/">home page</Link>.
                </p>
              </section>
            }
          />
        </Routes>
      </main>

      <footer className="site-footer">
        <p>
          Inspired by the open-source{" "}
          <a
            href="https://github.com/jacktrip/jacktrip"
            target="_blank"
            rel="noreferrer"
          >
            JackTrip project
          </a>{" "}
          from Stanford CCRMA.
        </p>
        <p>Dual-licensed under MIT and Apache&nbsp;2.0.</p>
      </footer>
    </div>
  );
}
