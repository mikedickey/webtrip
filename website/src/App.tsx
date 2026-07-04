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
          <a href={GITHUB_URL} target="_blank" rel="noreferrer">
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
