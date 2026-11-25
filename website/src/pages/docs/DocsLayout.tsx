import { NavLink, Outlet } from "react-router-dom";
import { API_REFERENCE_URL, DOCS } from "./content";
import "./docs.css";

export default function DocsLayout() {
  return (
    <div className="docs">
      <nav className="docs-nav" aria-label="Documentation">
        <ul>
          {DOCS.map((d) => (
            <li key={d.slug}>
              <NavLink to={`/docs/${d.slug}`}>{d.title}</NavLink>
            </li>
          ))}
          <li>
            <a href={API_REFERENCE_URL} target="_blank" rel="noreferrer">
              API reference ↗
            </a>
          </li>
        </ul>
      </nav>
      <div className="docs-content">
        <Outlet />
      </div>
    </div>
  );
}
