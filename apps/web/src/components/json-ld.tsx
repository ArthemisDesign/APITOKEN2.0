type JsonLdValue = Record<string, unknown> | Array<Record<string, unknown>>;

export function JsonLd({ data }: { data: JsonLdValue }) {
  return <script type="application/ld+json">{JSON.stringify(data).replace(/</g, "\\u003c")}</script>;
}
