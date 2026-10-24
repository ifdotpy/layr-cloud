export interface Route {
  page: string;
  project?: string;
  section?: string;
  id?: string;
  path?: string;
  line?: string;
  state?: string;
}
export function parseRoute(hash: string): Route {
  const [path, query = ""] = hash.replace(/^#\/?/, "").split("?");
  const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  const params = new URLSearchParams(query);
  if (parts[0] !== "project") return { page: parts[0] || "projects" };
  return {
    project: parts[1],
    page: parts[2] || "code",
    section: parts[3],
    id: parts[4],
    path: params.get("path") || "",
    line: params.get("line") || undefined,
    state: params.get("state") || undefined,
  };
}
export const projectUrl = (id: string, path = "code") =>
  `#/project/${encodeURIComponent(id)}/${path}`;
export const queryUrl = (
  url: string,
  params: Record<string, string | undefined>,
) => {
  const query = new URLSearchParams(
    Object.entries(params).filter(
      (entry): entry is [string, string] =>
        entry[1] !== undefined && entry[1] !== "",
    ),
  );
  return url + (query.size ? "?" + query.toString() : "");
};
