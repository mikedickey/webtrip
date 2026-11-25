import { DOCS, GITHUB_BLOB_URL } from "./content";

const PUBLISHED = new Set(DOCS.map((d) => d.slug));

export type ResolvedLink =
  | { kind: "route"; to: string }
  | { kind: "external"; href: string }
  | { kind: "anchor"; href: string };

// Resolve a link written for GitHub (relative to docs/) for the site: links to
// published guide pages stay inside the SPA, everything else in the repo goes
// to GitHub, and absolute URLs and same-page anchors pass through.
export function resolveDocLink(href: string): ResolvedLink {
  if (href.startsWith("#")) return { kind: "anchor", href };
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return { kind: "external", href };

  const [pathPart, hash = ""] = href.split("#", 2);
  const fragment = hash ? `#${hash}` : "";
  const repoPath = normalize(`docs/${pathPart}`);

  const page = /^docs\/([^/]+)\.md$/.exec(repoPath);
  if (page && PUBLISHED.has(page[1])) {
    return { kind: "route", to: `/docs/${page[1]}${fragment}` };
  }
  return { kind: "external", href: `${GITHUB_BLOB_URL}/${repoPath}${fragment}` };
}

function normalize(path: string): string {
  const out: string[] = [];
  for (const seg of path.split("/")) {
    if (seg === "..") out.pop();
    else if (seg !== "." && seg !== "") out.push(seg);
  }
  return out.join("/");
}
