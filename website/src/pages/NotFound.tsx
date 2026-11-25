import { Link } from "react-router-dom";

export default function NotFound() {
  return (
    <section className="page">
      <h1>Page not found</h1>
      <p className="lede">
        There&rsquo;s nothing at this address. Try the{" "}
        <Link to="/">home page</Link>.
      </p>
    </section>
  );
}
