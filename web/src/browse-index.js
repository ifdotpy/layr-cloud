// Directory indexes are optional. Their encrypted contents must match the requested state.
export const DIRECTORY_PAGE_SIZE = 256;
const integer = (n) => Number.isSafeInteger(n) && n >= 0;
const nameOK = (name) =>
  typeof name === "string" &&
  name.length > 0 &&
  ![".", ".."].includes(name) &&
  !/[/\0]/.test(name);
export function validatePath(path) {
  if (
    typeof path !== "string" ||
    (path !== "" && !path.split("/").every(nameOK))
  )
    throw new Error("Invalid folder path.");
}
function identity(value, state) {
  if (
    !value ||
    value.version !== 1 ||
    value.seq !== state.seq ||
    value.attempt !== state.attempt
  )
    throw new Error("The folder index does not match this version.");
}
export function validateMarker(value, state) {
  identity(value, state);
  if (typeof value.state !== "string" || !integer(value.files))
    throw new Error("Invalid folder index.");
  return value;
}
export function validateDirectory(value, state, path, page) {
  identity(value, state);
  validatePath(path);
  if (
    value.path !== path ||
    value.page !== page ||
    !integer(page) ||
    !integer(value.total) ||
    !Array.isArray(value.entries) ||
    value.entries.length > DIRECTORY_PAGE_SIZE ||
    value.entries.length > value.total ||
    (value.next !== null && value.next !== page + 1) ||
    (value.next !== null && value.entries.length !== DIRECTORY_PAGE_SIZE) ||
    page * DIRECTORY_PAGE_SIZE + value.entries.length > value.total ||
    (value.next === null &&
      page * DIRECTORY_PAGE_SIZE + value.entries.length !== value.total)
  )
    throw new Error("Invalid folder page.");
  const names = new Set();
  for (const item of value.entries) {
    if (!nameOK(item.name) || names.has(item.name))
      throw new Error("Invalid folder entry name.");
    names.add(item.name);
    if (item.kind === "directory") {
      if (!integer(item.count)) throw new Error("Invalid folder count.");
    } else if (item.kind === "file") {
      const e = item.entry;
      if (
        !Array.isArray(e) ||
        e.length !== 4 ||
        !["100644", "100755", "120000"].includes(e[0]) ||
        !integer(e[1]) ||
        !/^[0-9a-f]{64}$/.test(e[2]) ||
        !Array.isArray(e[3]) ||
        !e[3].length ||
        e[3].some(
          (link) =>
            !Array.isArray(link) ||
            link.length !== 6 ||
            !integer(link[0]) ||
            !/^[0-9a-f]{8}$/.test(link[1]) ||
            !link.slice(2, 5).every(integer) ||
            !["full", "delta"].includes(link[5]),
        )
      )
        throw new Error("Invalid file entry.");
    } else throw new Error("Invalid folder entry kind.");
  }
  return value;
}
