import { useEffect, useId, useState } from "react";

// Mermaid is large, so it's loaded only when a page actually has a diagram,
// and initialized once with the site's palette (the tokens in styles.css).
let mermaidReady: Promise<typeof import("mermaid").default> | undefined;

function loadMermaid() {
  mermaidReady ??= import("mermaid").then(({ default: mermaid }) => {
    const style = getComputedStyle(document.documentElement);
    const token = (name: string) => style.getPropertyValue(name).trim();
    const card = token("--bg-card");
    const border = token("--border-color");
    const text = token("--text-primary");
    const accent = token("--accent");
    mermaid.initialize({
      startOnLoad: false,
      theme: "base",
      // Mermaid measures text in this font to size boxes, so it must be a
      // concrete family rather than "inherit".
      fontFamily: getComputedStyle(document.body).fontFamily,
      // Tight margins so multi-lane sequence diagrams fit the docs column
      // without being scaled down to unreadable text.
      // Wrap flowchart labels wider than the 200px default, so a line of a
      // node label stays one line.
      flowchart: { wrappingWidth: 360 },
      sequence: {
        diagramMarginX: 8,
        actorMargin: 24,
        noteMargin: 8,
        boxTextMargin: 4,
      },
      themeVariables: {
        fontSize: "14px",
        darkMode: true,
        background: token("--bg-secondary"),
        primaryColor: card,
        primaryTextColor: text,
        primaryBorderColor: accent,
        lineColor: token("--text-secondary"),
        textColor: text,
        actorBkg: card,
        actorBorder: accent,
        actorTextColor: text,
        actorLineColor: border,
        signalColor: token("--accent-2"),
        signalTextColor: text,
        noteBkgColor: card,
        noteBorderColor: border,
        noteTextColor: text,
        clusterBkg: token("--bg-primary"),
        clusterBorder: border,
        edgeLabelBackground: token("--bg-secondary"),
      },
    });
    return mermaid;
  });
  return mermaidReady;
}

export default function Mermaid({ chart }: { chart: string }) {
  // useId() contains colons, which aren't valid in the id Mermaid gives the SVG.
  const id = `mermaid-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  // The outcome is tagged with the chart it belongs to, so when React reuses
  // this instance for a different chart (DocPage stays mounted across docs),
  // neither the old SVG nor an old failure outlives its chart.
  const [result, setResult] = useState<{ chart: string; svg?: string }>();

  useEffect(() => {
    let cancelled = false;
    loadMermaid()
      .then((mermaid) => mermaid.render(id, chart))
      .then(({ svg }) => {
        if (!cancelled) setResult({ chart, svg });
      })
      .catch((err) => {
        console.warn("Mermaid diagram failed to render", err);
        if (!cancelled) setResult({ chart });
      });
    return () => {
      cancelled = true;
    };
  }, [id, chart]);

  // Until this chart renders (or if it can't), show the diagram source.
  const svg = result?.chart === chart ? result.svg : undefined;
  if (!svg) {
    return (
      <pre>
        <code>{chart}</code>
      </pre>
    );
  }
  return <div className="docs-mermaid" dangerouslySetInnerHTML={{ __html: svg }} />;
}
