import { useEffect } from "react";
import { Link, useLocation, useParams } from "react-router-dom";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSlug from "rehype-slug";
import rehypeHighlight from "rehype-highlight";
import { DOCS } from "./content";
import { resolveDocLink } from "./links";
import NotFound from "../NotFound";
import Mermaid from "./Mermaid";

const components: Components = {
  a({ href = "", children, node: _node, ...rest }) {
    const link = resolveDocLink(href);
    if (link.kind === "route") {
      return (
        <Link to={link.to} {...rest}>
          {children}
        </Link>
      );
    }
    if (link.kind === "external") {
      return (
        <a href={link.href} target="_blank" rel="noreferrer" {...rest}>
          {children}
        </a>
      );
    }
    return (
      <a href={link.href} {...rest}>
        {children}
      </a>
    );
  },
  // Render ```mermaid fences as diagrams; every other fence stays a code block.
  pre({ node, children, ...rest }) {
    const code = node?.children[0];
    if (
      code?.type === "element" &&
      code.tagName === "code" &&
      (code.properties.className as string[] | undefined)?.includes("language-mermaid")
    ) {
      const source = code.children.map((c) => (c.type === "text" ? c.value : "")).join("");
      return <Mermaid chart={source} />;
    }
    return <pre {...rest}>{children}</pre>;
  },
  // Scroll wide tables inside their own box instead of widening the page.
  table({ node: _node, ...props }) {
    return (
      <div className="docs-table">
        <table {...props} />
      </div>
    );
  },
};

export default function DocPage() {
  const { slug } = useParams();
  const { hash } = useLocation();
  const doc = DOCS.find((d) => d.slug === slug);

  // Client-side navigation doesn't scroll to fragments by itself.
  useEffect(() => {
    if (!doc) return;
    const target = hash ? document.getElementById(decodeURIComponent(hash.slice(1))) : null;
    if (target) target.scrollIntoView();
    else window.scrollTo(0, 0);
  }, [doc, hash]);

  useEffect(() => {
    if (doc) document.title = `${doc.title} · WebTrip`;
  }, [doc]);

  if (!doc) return <NotFound />;

  return (
    <article className="prose">
      <Markdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeSlug, [rehypeHighlight, { detect: false, plainText: ["mermaid"] }]]}
        components={components}
      >
        {doc.markdown}
      </Markdown>
    </article>
  );
}
