// The guide pages, bundled from the repo's docs/ directory at build time. Only
// top-level docs/*.md are public; docs/contributing/ stays on GitHub.
const sources = import.meta.glob<string>("../../../../docs/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
});

export interface DocEntry {
  slug: string;
  title: string;
  markdown: string;
}

// Sidebar order. A slug is its filename without `.md`.
const ORDER: { slug: string; title: string }[] = [
  { slug: "getting-started", title: "Getting started" },
  { slug: "architecture", title: "Architecture" },
  { slug: "channel-model", title: "Channel model" },
  { slug: "browser-support", title: "Browser support" },
];

const bySlug = new Map(
  Object.entries(sources).map(([path, markdown]) => [
    path.slice(path.lastIndexOf("/") + 1, -".md".length),
    markdown,
  ]),
);

export const DOCS: DocEntry[] = ORDER.flatMap(({ slug, title }) => {
  const markdown = bySlug.get(slug);
  return markdown === undefined ? [] : [{ slug, title, markdown }];
});

export const API_REFERENCE_URL = "https://docs.rs/webtrip";

export const GITHUB_BLOB_URL = "https://github.com/mikedickey/webtrip/blob/main";
